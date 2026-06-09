use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::thread;

use crossbeam_channel::Sender;
use llama_chat_db::SharedDatabase;
use llama_chat_engine::generate_llama_response;
use llama_chat_types::models::{SharedLlamaState, TokenData};

use crate::mcp::McpManager;

use crate::worker::ipc_types::{WorkerPayload, WorkerResponse};

pub(super) struct GenerationParams {
    pub(super) req_id: u64,
    pub(super) user_message: String,
    pub(super) conversation_id: Option<String>,
    pub(super) skip_user_logging: bool,
    pub(super) image_data: Option<Vec<String>>,
    pub(super) agent_id: Option<String>,
    pub(super) llama_state: SharedLlamaState,
    pub(super) db: SharedDatabase,
    pub(super) cancel: Arc<AtomicBool>,
    pub(super) tx: Sender<WorkerResponse>,
    pub(super) mcp_manager: Arc<McpManager>,
}

pub(super) fn run_generation(params: GenerationParams) {
    use llama_chat_db::conversation::ConversationLogger;
    use llama_chat_engine::config_ext::get_resolved_system_prompt;
    use tokio::sync::mpsc;

    crate::prevent_sleep::retain();
    struct SleepGuard;
    impl Drop for SleepGuard {
        fn drop(&mut self) {
            crate::prevent_sleep::release();
        }
    }
    let _sleep_guard = SleepGuard;

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Failed to create tokio runtime for generation");

    let GenerationParams {
        req_id,
        user_message,
        conversation_id,
        skip_user_logging,
        image_data,
        agent_id,
        llama_state,
        db,
        cancel,
        tx,
        mcp_manager,
    } = params;

    rt.block_on(async {
        let shared_logger = if let Some(ref conv_id) = conversation_id {
            match ConversationLogger::from_existing(db.clone(), conv_id) {
                Ok(logger) => Arc::new(Mutex::new(logger)),
                Err(e) => {
                    let _ = tx.send(WorkerResponse::error(
                        req_id,
                        format!("Failed to load conversation: {e}"),
                    ));
                    return;
                }
            }
        } else {
            let system_prompt = get_resolved_system_prompt(&db, &Some(llama_state.clone()));
            match ConversationLogger::new(db.clone(), system_prompt.as_deref()) {
                Ok(logger) => Arc::new(Mutex::new(logger)),
                Err(e) => {
                    let _ = tx.send(WorkerResponse::error(
                        req_id,
                        format!("Failed to create conversation: {e}"),
                    ));
                    return;
                }
            }
        };

        {
            let conv_id = shared_logger.lock().unwrap().get_conversation_id();
            let _ = tx.send(WorkerResponse::ok(
                req_id,
                WorkerPayload::GenerationStarted {
                    conversation_id: conv_id,
                },
            ));
        }

        if !skip_user_logging {
            let mut logger = shared_logger.lock().unwrap();
            let estimated_tokens = (user_message.len() / 4).max(1) as i32;
            logger.log_message_with_tokens("USER", &user_message, Some(estimated_tokens));
        }

        let (token_sender, mut token_receiver) = mpsc::unbounded_channel::<TokenData>();
        let tx_clone = tx.clone();
        let forward_thread = thread::spawn(move || {
            loop {
                match token_receiver.blocking_recv() {
                    Some(token_data) => {
                        let response = WorkerResponse::ok(
                            req_id,
                            WorkerPayload::Token {
                                token: token_data.token,
                                tokens_used: token_data.tokens_used,
                                max_tokens: token_data.max_tokens,
                                status: token_data.status,
                                tool_timing: token_data.tool_timing,
                            },
                        );
                        if tx_clone.send(response).is_err() {
                            break;
                        }
                    }
                    None => break,
                }
            }
        });

        let result = generate_llama_response(
            &user_message,
            llama_state,
            shared_logger.clone(),
            Some(token_sender),
            true,
            db.clone(),
            cancel,
            image_data.as_deref(),
            Some(mcp_manager),
            agent_id.as_deref(),
        )
        .await;

        let _ = result.as_ref().ok();
        let _ = forward_thread.join();

        let final_conv_id = shared_logger
            .lock()
            .map(|l| l.get_conversation_id())
            .unwrap_or_default();

        match result {
            Ok(output) => {
                let _ = tx.send(WorkerResponse::ok(
                    req_id,
                    WorkerPayload::GenerationComplete {
                        conversation_id: final_conv_id,
                        tokens_used: output.tokens_used,
                        max_tokens: output.max_tokens,
                        prompt_tok_per_sec: output.prompt_tok_per_sec,
                        gen_tok_per_sec: output.gen_tok_per_sec,
                        gen_eval_ms: output.gen_eval_ms,
                        gen_tokens: output.gen_tokens,
                        prompt_eval_ms: output.prompt_eval_ms,
                        prompt_tokens: output.prompt_tokens,
                        finish_reason: Some(output.finish_reason),
                        token_breakdown: output.token_breakdown,
                    },
                ));
            }
            Err(e) if e == "Cancelled" => {
                let _ = tx.send(WorkerResponse::ok(
                    req_id,
                    WorkerPayload::GenerationCancelled,
                ));
            }
            Err(e) => {
                eprintln!("[WORKER] Generation error: {e}");
                let _ = tx.send(WorkerResponse::error(req_id, e));
            }
        }
    });
}
