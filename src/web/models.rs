use llama_cpp_2::{
    context::LlamaContext, llama_backend::LlamaBackend, model::LlamaModel,
    token::LlamaToken,
};
#[cfg(feature = "vision")]
use llama_cpp_2::mtmd::MtmdContext;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

use super::database::conversation::ConversationLogger;

/// Cached inference context for KV cache reuse across conversation turns.
///
/// SAFETY: The `context` field stores a `LlamaContext` whose lifetime is erased
/// to `'static`. The actual lifetime is tied to the `LlamaModel` in the parent
/// `LlamaState`. The context MUST be dropped (set to `None`) before the model
/// is dropped. This invariant is enforced by clearing `inference_cache` in
/// `model_manager.rs` before any model change or unload.
pub struct InferenceCache {
    /// Reusable LlamaContext with erased lifetime (model must outlive this).
    pub context: LlamaContext<'static>,
    /// The conversation this cache belongs to.
    pub conversation_id: String,
    /// Tokens currently evaluated in the KV cache.
    pub evaluated_tokens: Vec<LlamaToken>,
    /// Context size used when creating this context.
    pub context_size: u32,
    /// Whether GPU KV offload was enabled.
    pub offload_kqv: bool,
    /// Whether flash attention was enabled.
    pub flash_attention: bool,
    /// KV cache quantization type for K.
    pub cache_type_k: String,
    /// KV cache quantization type for V.
    pub cache_type_v: String,
}

// SAFETY: LlamaContext wraps a raw C pointer (NonNull) which is !Send by default.
// However, the llama.cpp context is not tied to a specific thread — it's safe to
// move between threads as long as it's not used concurrently. We guarantee
// single-threaded access via the Mutex<Option<LlamaState>> wrapper.
unsafe impl Send for InferenceCache {}

#[cfg(feature = "vision")]
/// Vision/multimodal context state. Wraps MtmdContext for Send safety.
/// MUST be dropped before the model (same invariant as InferenceCache).
pub struct VisionState {
    pub context: MtmdContext,
    pub mmproj_path: String,
}

#[cfg(feature = "vision")]
// SAFETY: Same as InferenceCache — MtmdContext wraps NonNull (!Send) but is safe
// to move between threads when not used concurrently (protected by Mutex).
unsafe impl Send for VisionState {}

// Import logging macros
use crate::sys_debug;

/// System prompt type selection
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[derive(Default)]
pub enum SystemPromptType {
    /// Use curated agentic system prompt with native tool tags
    #[default]
    Custom,
    /// User-defined manual prompt
    UserDefined,
}


// Configuration structure
#[derive(Deserialize, Serialize, Clone)]
pub struct SamplerConfig {
    pub sampler_type: String,
    pub temperature: f64,
    pub top_p: f64,
    pub top_k: u32,
    pub mirostat_tau: f64,
    pub mirostat_eta: f64,
    #[serde(default = "default_repeat_penalty")]
    pub repeat_penalty: f64,
    #[serde(default)]
    pub min_p: f64,
    // Extended sampling params
    #[serde(default = "default_typical_p")]
    pub typical_p: f64,
    #[serde(default)]
    pub frequency_penalty: f64,
    #[serde(default)]
    pub presence_penalty: f64,
    #[serde(default = "default_penalty_last_n")]
    pub penalty_last_n: i32,
    #[serde(default)]
    pub dry_multiplier: f64,
    #[serde(default = "default_dry_base")]
    pub dry_base: f64,
    #[serde(default = "default_dry_allowed_length")]
    pub dry_allowed_length: i32,
    #[serde(default = "default_dry_penalty_last_n")]
    pub dry_penalty_last_n: i32,
    #[serde(default = "default_top_n_sigma")]
    pub top_n_sigma: f64,
    // Advanced context params
    #[serde(default)]
    pub flash_attention: bool,
    #[serde(default = "default_cache_type")]
    pub cache_type_k: String,
    #[serde(default = "default_cache_type")]
    pub cache_type_v: String,
    #[serde(default = "default_n_batch")]
    pub n_batch: u32,
    pub model_path: Option<String>,
    pub system_prompt: Option<String>,
    #[serde(default)]
    pub system_prompt_type: SystemPromptType,
    pub context_size: Option<u32>,
    pub stop_tokens: Option<Vec<String>>,
    #[serde(default)]
    pub model_history: Vec<String>,
    #[serde(default = "default_true")]
    pub disable_file_logging: bool,
    // Tool tag overrides (None = use auto-detected)
    #[serde(default)]
    pub tool_tag_exec_open: Option<String>,
    #[serde(default)]
    pub tool_tag_exec_close: Option<String>,
    #[serde(default)]
    pub tool_tag_output_open: Option<String>,
    #[serde(default)]
    pub tool_tag_output_close: Option<String>,
    // App settings
    #[serde(default)]
    pub web_search_provider: Option<String>,
    #[serde(default)]
    pub web_search_api_key: Option<String>,
    // Hardware / context / sampler params
    #[serde(default = "default_seed")]
    pub seed: i32,
    #[serde(default = "default_n_ubatch")]
    pub n_ubatch: u32,
    #[serde(default)]
    pub n_threads: i32,
    #[serde(default)]
    pub n_threads_batch: i32,
    #[serde(default)]
    pub rope_freq_base: f32,
    #[serde(default)]
    pub rope_freq_scale: f32,
    #[serde(default)]
    pub use_mlock: bool,
    #[serde(default = "default_true")]
    pub use_mmap: bool,
    #[serde(default)]
    pub main_gpu: i32,
    #[serde(default = "default_split_mode")]
    pub split_mode: String,
    #[serde(default)]
    pub tag_pairs: Option<Vec<crate::web::chat::tool_tags::TagPair>>,
}

fn default_true() -> bool {
    true
}

fn default_repeat_penalty() -> f64 {
    1.0
}

fn default_cache_type() -> String {
    "f16".to_string()
}

fn default_n_batch() -> u32 {
    2048
}

fn default_typical_p() -> f64 {
    1.0
}

fn default_penalty_last_n() -> i32 {
    64
}

fn default_dry_base() -> f64 {
    1.75
}

fn default_dry_allowed_length() -> i32 {
    2
}

fn default_dry_penalty_last_n() -> i32 {
    -1
}

fn default_top_n_sigma() -> f64 {
    -1.0
}

fn default_seed() -> i32 {
    -1
}

fn default_n_ubatch() -> u32 {
    512
}

fn default_split_mode() -> String {
    "layer".to_string()
}

// Common stop tokens for different model providers
pub fn get_common_stop_tokens() -> Vec<String> {
    vec![
        // Generic end-of-sequence tokens (model specific)
        "<|end_of_text|>".to_string(),
        "<|eot_id|>".to_string(),
        "<|im_end|>".to_string(),
        "<|endoftext|>".to_string(),
        "</s>".to_string(),
        "<end_of_turn>".to_string(),
        "<|end|>".to_string(),        // Phi-3/Phi-4 turn separator
        "<|user|>".to_string(),        // GLM/Phi role boundary (stop before next user turn)
        "<|observation|>".to_string(), // GLM tool result boundary
        "<|system|>".to_string(),      // GLM/Phi system role (model hallucinating turns)
        "<|assistant|>".to_string(),   // GLM/Phi assistant role (model hallucinating turns)
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
            repeat_penalty: 1.0,
            min_p: 0.0,
            typical_p: 1.0,
            frequency_penalty: 0.0,
            presence_penalty: 0.0,
            penalty_last_n: 64,
            dry_multiplier: 0.0,
            dry_base: 1.75,
            dry_allowed_length: 2,
            dry_penalty_last_n: -1,
            top_n_sigma: -1.0,
            flash_attention: true,
            cache_type_k: "f16".to_string(),
            cache_type_v: "f16".to_string(),
            n_batch: 2048,
            model_path: Some("/app/models/lmstudio-community/granite-4.0-h-tiny-GGUF/granite-4.0-h-tiny-Q4_K_M.gguf".to_string()),
            system_prompt: None,
            system_prompt_type: SystemPromptType::default(),
            context_size: Some(32768),
            stop_tokens: Some(get_common_stop_tokens()),
            model_history: Vec::new(),
            disable_file_logging: true,
            tool_tag_exec_open: None,
            tool_tag_exec_close: None,
            tool_tag_output_open: None,
            tool_tag_output_close: None,
            web_search_provider: None,
            web_search_api_key: None,
            seed: -1,
            n_ubatch: 512,
            n_threads: 0,
            n_threads_batch: 0,
            rope_freq_base: 0.0,
            rope_freq_scale: 0.0,
            use_mlock: false,
            use_mmap: true,
            main_gpu: 0,
            split_mode: "layer".to_string(),
            tag_pairs: None,
        }
    }
}

// Model capabilities for tool calling compatibility
#[derive(Debug, Clone)]
pub struct ModelCapabilities {
    pub native_file_tools: bool, // Can use read_file, write_file, list_directory natively
    pub bash_tool: bool,         // Can use bash tool
    #[allow(dead_code)]
    pub requires_translation: bool, // Needs file tools translated to bash
}

impl Default for ModelCapabilities {
    fn default() -> Self {
        ModelCapabilities {
            native_file_tools: true,
            bash_tool: true,
            requires_translation: false,
        }
    }
}

/// Determine model capabilities based on chat template type
pub fn get_model_capabilities(chat_template: &str) -> ModelCapabilities {
    match chat_template {
        // Qwen/ChatML models refuse file tools due to safety training
        // but accept bash commands - so we translate file ops to bash
        "ChatML" => ModelCapabilities {
            native_file_tools: false,
            bash_tool: true,
            requires_translation: true,
        },

        // Mistral/Devstral models support all tools natively
        "Mistral" | "Devstral" => ModelCapabilities {
            native_file_tools: true,
            bash_tool: true,
            requires_translation: false,
        },

        // Llama3 models - assume they work like Mistral (can be updated after testing)
        "Llama3" => ModelCapabilities {
            native_file_tools: true,
            bash_tool: true,
            requires_translation: false,
        },

        // GLM models support native tool calling with <tool_call> format
        "GLM" => ModelCapabilities {
            native_file_tools: true,
            bash_tool: true,
            requires_translation: false,
        },

        // Unknown models - default to safe assumption (use bash translation)
        _ => ModelCapabilities {
            native_file_tools: false,
            bash_tool: true,
            requires_translation: true,
        },
    }
}

/// Translate tool calls to bash commands for models that don't support native file tools
pub fn translate_tool_for_model(
    tool_name: &str,
    arguments: &serde_json::Value,
    capabilities: &ModelCapabilities,
) -> (String, serde_json::Value) {
    // If model doesn't support native file tools, translate to bash
    if !capabilities.native_file_tools && capabilities.bash_tool {
        match tool_name {
            "read_file" => {
                let path = arguments.get("path").and_then(|v| v.as_str()).unwrap_or("");

                let command = if cfg!(target_os = "windows") {
                    // PowerShell: Use Get-Content (cat is an alias)
                    // PowerShell handles Windows paths with backslashes correctly
                    format!("cat \"{path}\"")
                } else {
                    format!("cat \"{path}\"")
                };

                sys_debug!("[TOOL TRANSLATION] read_file → bash: {}", command);

                ("bash".to_string(), serde_json::json!({"command": command}))
            }

            "write_file" => {
                let path = arguments.get("path").and_then(|v| v.as_str()).unwrap_or("");
                let content = arguments
                    .get("content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                let command = if cfg!(target_os = "windows") {
                    // PowerShell: Use Set-Content or Out-File
                    // Note: echo in PowerShell automatically writes to file with >
                    format!(
                        "'{content}' | Out-File -FilePath \"{path}\" -Encoding UTF8"
                    )
                } else {
                    // Linux/Mac: echo 'content' > "file"
                    format!("echo '{content}' > \"{path}\"")
                };

                sys_debug!("[TOOL TRANSLATION] write_file → bash: {}", command);

                ("bash".to_string(), serde_json::json!({"command": command}))
            }

            "list_directory" => {
                let path = arguments
                    .get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or(".");
                let recursive = arguments
                    .get("recursive")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                let command = if cfg!(target_os = "windows") {
                    // PowerShell: Use Get-ChildItem (ls is an alias)
                    if recursive {
                        format!("ls -Recurse \"{path}\"")
                    } else {
                        format!("ls \"{path}\"")
                    }
                } else if recursive {
                    format!("ls -R \"{path}\"")
                } else {
                    format!("ls -la \"{path}\"")
                };

                sys_debug!("[TOOL TRANSLATION] list_directory → bash: {}", command);

                ("bash".to_string(), serde_json::json!({"command": command}))
            }

            // All other tools pass through unchanged
            _ => (tool_name.to_string(), arguments.clone()),
        }
    } else {
        // Model supports native tools - no translation needed
        (tool_name.to_string(), arguments.clone())
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
    /// Base64-encoded image data URIs (e.g., "data:image/png;base64,...") for vision models.
    /// Supports multiple images per message.
    #[serde(default)]
    pub image_data: Option<Vec<String>>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_vision: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_tags: Option<crate::web::chat::tool_tags::ToolTags>,
}

#[derive(Deserialize)]
pub struct ModelLoadRequest {
    pub model_path: String,
    pub gpu_layers: Option<u32>,
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
    pub chat_template_string: Option<String>, // Store full Jinja2 template from model
    pub gpu_layers: Option<u32>,            // Number of GPU layers offloaded
    pub last_used: std::time::SystemTime,
    pub general_name: Option<String>,       // Model's general.name from GGUF metadata
    // Cached resolved system prompt (invalidated on config or model change)
    pub cached_system_prompt: Option<String>,
    pub cached_prompt_key: Option<(Option<String>, Option<String>)>, // (system_prompt, general_name)
    /// Cached inference context for KV cache reuse. MUST be dropped before model.
    pub inference_cache: Option<InferenceCache>,
    #[cfg(feature = "vision")]
    /// Vision/multimodal context (if mmproj loaded). MUST be dropped before model.
    pub vision_state: Option<VisionState>,
}

pub type SharedLlamaState = Arc<Mutex<Option<LlamaState>>>;

pub type SharedConversationLogger = Arc<Mutex<ConversationLogger>>;
