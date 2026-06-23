use super::ToolTags;
use serde::{Deserialize, Serialize};

/// One typed segment of a message (text, tool_call, tool_result, reasoning).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MessagePart {
    #[serde(rename = "type")]
    pub part_type: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_args: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct ToolTimingLive {
    pub name: String,
    pub duration_ms: u64,
}

/// Carries an approval request to the frontend for dangerous tool calls.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ApprovalRequest {
    pub id: String,
    pub tool: String,
    pub args: serde_json::Value,
    pub reason: String,
}

#[derive(Serialize, Clone, Default)]
pub struct TokenData {
    pub token: String,
    pub tokens_used: i32,
    pub max_tokens: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gen_tok_per_sec: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gen_tokens: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_timing: Option<ToolTimingLive>,
    /// When present, the frontend should pause and show an approve/reject dialog.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approval_required: Option<ApprovalRequest>,
}

#[derive(Deserialize)]
pub struct ChatRequest {
    pub message: String,
    pub conversation_id: Option<String>,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub worker_id: Option<String>,
    #[serde(default)]
    pub image_data: Option<Vec<String>>,
    #[serde(default)]
    pub auto_continue: bool,
    /// True when the client is reconnecting after a dropped connection and the
    /// server should NOT start a new generation — just wait for the in-progress
    /// one to finish and send a synthetic done event.
    #[serde(default)]
    pub reconnect: bool,
}

#[derive(Serialize)]
pub struct ChatResponse {
    pub message: ChatMessage,
    pub conversation_id: String,
    pub tokens_used: Option<i32>,
    pub max_tokens: Option<i32>,
}

#[derive(Serialize)]
pub struct ChatMessage {
    pub id: String,
    pub role: String,
    pub content: String,
    pub timestamp: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_tok_per_sec: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gen_tok_per_sec: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gen_eval_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gen_tokens: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_eval_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_tokens: Option<i32>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub compacted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sequence_order: Option<i32>,
    /// Structured parts (remote provider messages). Empty for local-model messages.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub parts: Vec<MessagePart>,
    /// LLM-generated short title (≤50 chars). Present on user messages after background gen.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

#[derive(Serialize)]
pub struct ConversationFile {
    pub name: String,
    pub display_name: String,
    pub timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worker_id: Option<String>,
}

#[derive(Serialize)]
pub struct ConversationsResponse {
    pub conversations: Vec<ConversationFile>,
}

#[derive(Serialize)]
pub struct ToolTiming {
    pub name: String,
    pub duration_ms: u64,
}

#[derive(Serialize)]
pub struct ConversationContentResponse {
    pub content: String,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worker_id: Option<String>,
    pub tool_timings: Vec<ToolTiming>,
}

#[derive(Serialize)]
pub struct FileItem {
    pub name: String,
    pub path: String,
    pub is_directory: bool,
    pub size: Option<u64>,
}

#[derive(Serialize)]
pub struct BrowseFilesResponse {
    pub files: Vec<FileItem>,
    pub current_path: String,
    pub parent_path: Option<String>,
}

#[derive(Serialize)]
pub struct ModelStatus {
    pub loaded: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loading: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loading_progress: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generating: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_conversation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_message: Option<String>,
    pub model_path: Option<String>,
    pub last_used: Option<String>,
    pub memory_usage_mb: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_vision: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_tags: Option<ToolTags>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu_layers: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_prompt_tokens: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_definitions_tokens: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_size: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_finish_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_thinking: Option<bool>,
    /// True when the status reflects an agent/conversation worker (not the default worker).
    /// EmptyChat uses this to avoid showing the model-name heading when no agent is staged.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_agent_model: Option<bool>,
}

#[derive(Deserialize)]
pub struct ModelLoadRequest {
    pub model_path: String,
    pub gpu_layers: Option<u32>,
    pub mmproj_path: Option<String>,
    pub context_size: Option<u32>,
    pub flash_attention: Option<bool>,
    pub cache_type_k: Option<String>,
    pub cache_type_v: Option<String>,
}

#[derive(Serialize)]
pub struct ModelResponse {
    pub success: bool,
    pub message: String,
    pub status: Option<ModelStatus>,
}
