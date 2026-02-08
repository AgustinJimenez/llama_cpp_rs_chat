//! Generation request queue with cancellation support.
//!
//! Routes submit `GenerationRequest`s to a bounded MPSC channel.
//! A single worker task processes them sequentially via `spawn_blocking`,
//! ensuring only one generation runs at a time without callers blocking
//! on the model mutex directly.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, oneshot};

use super::chat::{generate_llama_response, GenerationOutput};
use super::database::SharedDatabase;
use super::models::{SharedConversationLogger, SharedLlamaState, TokenData};

/// Flag that the worker checks periodically to abort generation early.
pub type CancellationFlag = Arc<AtomicBool>;

/// Everything needed to run one generation call.
pub struct GenerationRequest {
    pub user_message: String,
    pub llama_state: SharedLlamaState,
    pub conversation_logger: SharedConversationLogger,
    pub token_sender: Option<mpsc::UnboundedSender<TokenData>>,
    pub skip_user_logging: bool,
    pub db: SharedDatabase,
    /// Caller sets this to `true` to request early stop.
    pub cancel: CancellationFlag,
    /// One-shot channel to deliver the final result back to the caller.
    pub result_sender: oneshot::Sender<Result<GenerationOutput, String>>,
}

/// Cloneable handle that route handlers use to submit generation work.
#[derive(Clone)]
pub struct GenerationQueue {
    tx: mpsc::Sender<GenerationRequest>,
    /// The cancellation flag of the currently in-progress generation (if any).
    active_cancel: Arc<Mutex<Option<CancellationFlag>>>,
}

pub type SharedGenerationQueue = Arc<GenerationQueue>;

impl GenerationQueue {
    /// Create the queue and spawn the background worker.
    pub fn spawn(capacity: usize) -> Self {
        let (tx, rx) = mpsc::channel::<GenerationRequest>(capacity);
        let active_cancel: Arc<Mutex<Option<CancellationFlag>>> = Arc::new(Mutex::new(None));

        tokio::spawn(generation_worker(rx, active_cancel.clone()));

        Self { tx, active_cancel }
    }

    /// Submit a generation request. Waits if the queue is full.
    pub async fn submit(&self, request: GenerationRequest) -> Result<(), String> {
        self.tx
            .send(request)
            .await
            .map_err(|_| "Generation queue closed".to_string())
    }

    /// Cancel the currently in-progress generation (if any).
    pub fn cancel_active(&self) {
        if let Ok(guard) = self.active_cancel.lock() {
            if let Some(ref flag) = *guard {
                flag.store(true, Ordering::SeqCst);
            }
        }
    }
}

/// Long-lived task that pulls requests off the channel one at a time.
async fn generation_worker(
    mut rx: mpsc::Receiver<GenerationRequest>,
    active_cancel: Arc<Mutex<Option<CancellationFlag>>>,
) {
    while let Some(req) = rx.recv().await {
        // Skip requests that were already cancelled before we got to them.
        if req.cancel.load(Ordering::SeqCst) {
            let _ = req
                .result_sender
                .send(Err("Cancelled before starting".to_string()));
            continue;
        }

        // Publish this request's flag so cancel_active() can reach it.
        {
            if let Ok(mut guard) = active_cancel.lock() {
                *guard = Some(req.cancel.clone());
            }
        }

        let cancel = req.cancel.clone();

        // Heavy work goes on the blocking thread pool.
        let join_result = tokio::task::spawn_blocking(move || {
            let rt = tokio::runtime::Handle::current();
            rt.block_on(generate_llama_response(
                &req.user_message,
                req.llama_state,
                req.conversation_logger,
                req.token_sender,
                req.skip_user_logging,
                &req.db,
                cancel,
            ))
        })
        .await;

        // Clear the active flag.
        {
            if let Ok(mut guard) = active_cancel.lock() {
                *guard = None;
            }
        }

        let final_result = match join_result {
            Ok(inner) => inner,
            Err(e) => Err(format!("Generation task panicked: {e}")),
        };

        // Caller may have dropped the receiver (disconnected) — ignore error.
        let _ = req.result_sender.send(final_result);
    }
}
