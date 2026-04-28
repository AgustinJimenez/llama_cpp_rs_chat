//! IPC protocol types for server <-> worker communication.
//!
//! Uses JSON Lines (one JSON object per line) over stdin/stdout pipes.

use serde::{Deserialize, Serialize};

use crate::event_log::ConversationEvent;
use crate::models::TokenBreakdown;

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
        mmproj_path: Option<String>,
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
    /// Generate a short title for a conversation (no conversation logging).
    GenerateTitle {
        conversation_id: String,
        prompt: String,
    },
    /// Refresh MCP server connections (reconnect + rediscover tools).
    RefreshMcpServers,
    /// Get current MCP status (connected servers, discovered tools).
    GetMcpStatus,
    /// Get conversation event log (stalls, compaction, tool calls, etc.).
    GetConversationEvents { conversation_id: String },
    /// Get global status message (compaction progress visible during generation).
    GetGlobalStatus,
    /// List available compute backends (CUDA, Vulkan, CPU, etc.).
    GetAvailableBackends,
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
        block_count: Option<u32>,
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
    /// Notification that generation started with a specific conversation ID.
    GenerationStarted {
        conversation_id: String,
    },
    /// Conversation event log response.
    ConversationEvents {
        events: Vec<ConversationEvent>,
    },
    /// Global status message response.
    GlobalStatus {
        status: Option<String>,
    },
    /// A streaming token during generation.
    Token {
        token: String,
        tokens_used: i32,
        max_tokens: i32,
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<String>,
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
        /// Why generation stopped: "stop", "length", "cancelled", "tool_calls", "error".
        finish_reason: Option<String>,
        /// Token usage breakdown by category.
        #[serde(skip_serializing_if = "Option::is_none")]
        token_breakdown: Option<TokenBreakdown>,
    },
    /// Generation was cancelled by the user.
    GenerationCancelled,
    /// Title generated for a conversation.
    TitleGenerated {
        conversation_id: String,
        title: String,
    },
    /// MCP servers refreshed successfully.
    McpServersRefreshed {
        connected_servers: Vec<String>,
        total_tools: usize,
    },
    /// Current MCP status.
    McpStatus {
        servers: Vec<McpServerStatus>,
    },
    /// Model loading progress update (0-100).
    LoadingProgress { progress: u8 },
    /// Health check response.
    Pong,
    /// Available compute backends.
    AvailableBackends {
        backends: Vec<BackendInfo>,
    },
    /// An error occurred.
    Error { message: String },
}

/// A compute backend (e.g. CUDA, Vulkan, CPU).
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BackendInfo {
    pub name: String,
    pub available: bool,
    pub devices: Vec<BackendDeviceInfo>,
}

/// A device within a compute backend.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BackendDeviceInfo {
    pub name: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vram_mb: Option<u64>,
}

/// Status of an individual MCP server.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct McpServerStatus {
    pub id: String,
    pub name: String,
    pub connected: bool,
    pub tool_count: usize,
    pub tools: Vec<String>,
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
