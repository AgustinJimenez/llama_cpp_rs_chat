use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use llama_cpp_2::{
    llama_backend::LlamaBackend,
    model::LlamaModel,
};

use super::conversation::ConversationLogger;

// Configuration structure
#[derive(Deserialize, Serialize, Clone)]
pub struct SamplerConfig {
    pub sampler_type: String,
    pub temperature: f64,
    pub top_p: f64,
    pub top_k: u32,
    pub mirostat_tau: f64,
    pub mirostat_eta: f64,
    pub model_path: Option<String>,
    pub system_prompt: Option<String>,
    pub context_size: Option<u32>,
    pub stop_tokens: Option<Vec<String>>,
    #[serde(default)]
    pub model_history: Vec<String>,
}

// Common stop tokens for different model providers
pub fn get_common_stop_tokens() -> Vec<String> {
    vec![
        // Custom command tag - stop AFTER the command is complete
        "</COMMAND>".to_string(),

        // LLaMA 3+ tokens
        "<|end_of_text|>".to_string(),
        "<|eot_id|>".to_string(),
        "<|start_header_id|>".to_string(),
        "<|end_header_id|>".to_string(),

        // Qwen tokens
        "<|im_start|>".to_string(),
        "<|im_end|>".to_string(),
        "<|endoftext|>".to_string(),

        // Mistral/Mixtral tokens
        "[INST]".to_string(),
        "[/INST]".to_string(),
        "</s>".to_string(),

        // Phi tokens
        "<|user|>".to_string(),
        "<|assistant|>".to_string(),
        "<|end|>".to_string(),
        "<|system|>".to_string(),

        // Gemma tokens
        "<start_of_turn>".to_string(),
        "<end_of_turn>".to_string(),

        // Generic role tokens
        "<|start_of_role|>".to_string(),
        "<|end_of_role|>".to_string(),
    ]
}

impl Default for SamplerConfig {
    fn default() -> Self {
        // Set system_prompt to None by default to use the model's built-in chat template
        // Users can customize the system prompt via the UI if needed
        // Note: get_available_tools_json() can provide tool definitions when needed
        Self {
            sampler_type: "Temperature".to_string(), // Use Temperature instead of Greedy
            temperature: 0.7,
            top_p: 0.95,
            top_k: 20,
            mirostat_tau: 5.0,
            mirostat_eta: 0.1,
            model_path: Some("/app/models/lmstudio-community/granite-4.0-h-tiny-GGUF/granite-4.0-h-tiny-Q4_K_M.gguf".to_string()),
            system_prompt: None,
            context_size: Some(32768),
            stop_tokens: Some(get_common_stop_tokens()),
            model_history: Vec::new(),
        }
    }
}

// Token data with metadata for streaming
#[derive(Serialize, Clone)]
pub struct TokenData {
    pub token: String,
    pub tokens_used: i32,
    pub max_tokens: i32,
}

// Request/Response structures
#[derive(Deserialize)]
pub struct ChatRequest {
    pub message: String,
    pub conversation_id: Option<String>,
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
}

#[derive(Serialize)]
pub struct ConversationFile {
    pub name: String,
    pub display_name: String,
    pub timestamp: String,
}

#[derive(Serialize)]
pub struct ConversationsResponse {
    pub conversations: Vec<ConversationFile>,
}

#[derive(Serialize)]
pub struct ConversationContentResponse {
    pub content: String,
    pub messages: Vec<ChatMessage>,
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
    pub model_path: Option<String>,
    pub last_used: Option<String>,
    pub memory_usage_mb: Option<u64>,
}

#[derive(Deserialize)]
pub struct ModelLoadRequest {
    pub model_path: String,
}

#[derive(Serialize)]
pub struct ModelResponse {
    pub success: bool,
    pub message: String,
    pub status: Option<ModelStatus>,
}

// Shared state for LLaMA
pub struct LlamaState {
    pub backend: LlamaBackend,
    pub model: Option<LlamaModel>,
    pub current_model_path: Option<String>,
    pub model_context_length: Option<u32>,
    pub chat_template_type: Option<String>, // Store detected template type
    pub last_used: std::time::SystemTime,
}

pub type SharedLlamaState = Arc<Mutex<Option<LlamaState>>>;

pub type SharedConversationLogger = Arc<Mutex<ConversationLogger>>;
