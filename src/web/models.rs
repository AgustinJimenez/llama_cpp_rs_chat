use llama_cpp_2::{
    context::LlamaContext, llama_backend::LlamaBackend, model::LlamaModel, token::LlamaToken,
};
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
}

// SAFETY: LlamaContext wraps a raw C pointer (NonNull) which is !Send by default.
// However, the llama.cpp context is not tied to a specific thread — it's safe to
// move between threads as long as it's not used concurrently. We guarantee
// single-threaded access via the Mutex<Option<LlamaState>> wrapper.
unsafe impl Send for InferenceCache {}

// Import logging macros
use crate::sys_debug;

/// System prompt type selection
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[derive(Default)]
pub enum SystemPromptType {
    /// Use model's native Jinja2 chat template
    #[default]
    Default,
    /// Use custom curated prompts
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
    pub model_path: Option<String>,
    pub system_prompt: Option<String>,
    #[serde(default)]
    pub system_prompt_type: SystemPromptType,
    pub context_size: Option<u32>,
    pub stop_tokens: Option<Vec<String>>,
    #[serde(default)]
    pub model_history: Vec<String>,
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
            system_prompt_type: SystemPromptType::default(),
            context_size: Some(32768),
            stop_tokens: Some(get_common_stop_tokens()),
            model_history: Vec::new(),
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
    pub chat_template_string: Option<String>, // Store full Jinja2 template from model
    pub gpu_layers: Option<u32>,            // Number of GPU layers offloaded
    pub last_used: std::time::SystemTime,
    pub model_default_system_prompt: Option<String>, // Model's default system prompt from GGUF
    pub general_name: Option<String>,       // Model's general.name from GGUF metadata
    // Cached resolved system prompt (invalidated on config or model change)
    pub cached_system_prompt: Option<String>,
    pub cached_prompt_key: Option<(Option<String>, Option<String>)>, // (system_prompt, general_name)
    /// Cached inference context for KV cache reuse. MUST be dropped before model.
    pub inference_cache: Option<InferenceCache>,
}

pub type SharedLlamaState = Arc<Mutex<Option<LlamaState>>>;

pub type SharedConversationLogger = Arc<Mutex<ConversationLogger>>;
