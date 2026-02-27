//! IPC protocol types for server ↔ worker communication.
//!
//! Uses JSON Lines (one JSON object per line) over stdin/stdout pipes.

use serde::{Deserialize, Serialize};

/// Request sent from server to worker via stdin.
#[derive(Serialize, Deserialize, Debug)]
pub struct WorkerRequest {
    /// Monotonic request ID for correlating responses. 0 = fire-and-forget.
    pub id: u64,
    pub command: WorkerCommand,
}

/// Commands the server can send to the worker.
#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type")]
pub enum WorkerCommand {
    /// Load a GGUF model file.
    LoadModel {
        model_path: String,
        gpu_layers: Option<u32>,
    },
    /// Unload the current model (free memory within the process).
    UnloadModel,
    /// Get current model status.
    GetModelStatus,
    /// Start a generation request.
    Generate {
        user_message: String,
        conversation_id: Option<String>,
        skip_user_logging: bool,
        /// Base64-encoded image data URIs for vision models (supports multiple).
        #[serde(default)]
        image_data: Option<Vec<String>>,
    },
    /// Cancel the in-progress generation.
    CancelGeneration,
    /// Health check.
    Ping,
    /// Graceful shutdown.
    Shutdown,
}

/// Response sent from worker to server via stdout.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WorkerResponse {
    /// Matches the request ID. 0 for unsolicited messages.
    pub id: u64,
    pub payload: WorkerPayload,
}

/// Response payloads from the worker.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type")]
pub enum WorkerPayload {
    /// Model loaded successfully.
    ModelLoaded {
        model_path: String,
        context_length: Option<u32>,
        chat_template_type: Option<String>,
        chat_template_string: Option<String>,
        gpu_layers: Option<u32>,
        general_name: Option<String>,
        has_vision: Option<bool>,
    },
    /// Model unloaded.
    ModelUnloaded,
    /// Current model status.
    ModelStatus {
        loaded: bool,
        model_path: Option<String>,
        general_name: Option<String>,
        context_length: Option<u32>,
        gpu_layers: Option<u32>,
    },
    /// A streaming token during generation.
    Token {
        token: String,
        tokens_used: i32,
        max_tokens: i32,
    },
    /// Generation completed successfully.
    GenerationComplete {
        conversation_id: String,
        tokens_used: i32,
        max_tokens: i32,
        /// Prompt evaluation speed (tokens/second).
        prompt_tok_per_sec: Option<f64>,
        /// Generation speed (tokens/second).
        gen_tok_per_sec: Option<f64>,
        /// Generation time in milliseconds.
        gen_eval_ms: Option<f64>,
        /// Number of tokens generated.
        gen_tokens: Option<i32>,
        /// Prompt evaluation time in milliseconds.
        prompt_eval_ms: Option<f64>,
        /// Number of prompt tokens evaluated.
        prompt_tokens: Option<i32>,
    },
    /// Generation was cancelled by the user.
    GenerationCancelled,
    /// Health check response.
    Pong,
    /// An error occurred.
    Error { message: String },
}

impl WorkerResponse {
    pub fn ok(id: u64, payload: WorkerPayload) -> Self {
        Self { id, payload }
    }

    pub fn error(id: u64, message: impl Into<String>) -> Self {
        Self {
            id,
            payload: WorkerPayload::Error {
                message: message.into(),
            },
        }
    }
}
