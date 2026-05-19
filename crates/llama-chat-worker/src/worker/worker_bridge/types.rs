//! Types used by the WorkerBridge: metadata, pending requests, generation state, results.

use std::sync::Arc;

use tokio::sync::{mpsc, oneshot, Mutex as TokioMutex};

use super::super::ipc_types::*;
use llama_chat_types::models::TokenData;

/// Cached model metadata from the worker.
#[derive(Debug, Clone)]
pub struct ModelMeta {
    pub loaded: bool,
    pub model_path: String,
    pub context_length: Option<u32>,
    pub chat_template_type: Option<String>,
    pub general_name: Option<String>,
    pub has_vision: bool,
    pub gpu_layers: Option<u32>,
    pub block_count: Option<u32>,
    pub supports_thinking: bool,
}

/// A pending request awaiting a response from the worker.
pub struct PendingRequest {
    pub tx: oneshot::Sender<WorkerPayload>,
}

/// An active streaming generation.
pub struct ActiveGeneration {
    pub request_id: u64,
    pub token_tx: mpsc::UnboundedSender<TokenData>,
    pub conversation_id: Option<String>,
}

/// Result of a completed generation.
#[derive(Debug)]
#[allow(dead_code)]
pub enum GenerationResult {
    Complete {
        conversation_id: String,
        tokens_used: i32,
        max_tokens: i32,
        prompt_tok_per_sec: Option<f64>,
        gen_tok_per_sec: Option<f64>,
        gen_eval_ms: Option<f64>,
        gen_tokens: Option<i32>,
        prompt_eval_ms: Option<f64>,
        prompt_tokens: Option<i32>,
        finish_reason: Option<String>,
        token_breakdown: Option<llama_chat_types::models::TokenBreakdown>,
    },
    Cancelled,
    Error(String),
}

/// Adapt a GenerationResult oneshot into a WorkerPayload oneshot for the pending map.
pub(super) fn oneshot_adapter(
    done_tx: oneshot::Sender<GenerationResult>,
    active_gen: Arc<TokioMutex<Option<ActiveGeneration>>>,
    finish_reason_store: Arc<TokioMutex<Option<String>>>,
) -> oneshot::Sender<WorkerPayload> {
    let (payload_tx, payload_rx) = oneshot::channel::<WorkerPayload>();

    tokio::spawn(async move {
        if let Ok(payload) = payload_rx.await {
            // Clear active generation
            *active_gen.lock().await = None;

            let result = match payload {
                WorkerPayload::GenerationComplete {
                    conversation_id,
                    tokens_used,
                    max_tokens,
                    prompt_tok_per_sec,
                    gen_tok_per_sec,
                    gen_eval_ms,
                    gen_tokens,
                    prompt_eval_ms,
                    prompt_tokens,
                    finish_reason,
                    token_breakdown,
                } => {
                    // Store finish_reason for polling-based auto-continue
                    *finish_reason_store.lock().await = finish_reason.clone();
                    GenerationResult::Complete {
                        conversation_id,
                        tokens_used,
                        max_tokens,
                        prompt_tok_per_sec,
                        gen_tok_per_sec,
                        gen_eval_ms,
                        gen_tokens,
                        prompt_eval_ms,
                        prompt_tokens,
                        finish_reason,
                        token_breakdown,
                    }
                }
                WorkerPayload::GenerationCancelled => {
                    *finish_reason_store.lock().await = Some("cancelled".to_string());
                    GenerationResult::Cancelled
                }
                WorkerPayload::Error { message } => {
                    *finish_reason_store.lock().await = Some("error".to_string());
                    GenerationResult::Error(message)
                }
                _ => {
                    *finish_reason_store.lock().await = Some("error".to_string());
                    GenerationResult::Error("Unexpected response".to_string())
                }
            };
            let _ = done_tx.send(result);
        }
    });

    payload_tx
}
