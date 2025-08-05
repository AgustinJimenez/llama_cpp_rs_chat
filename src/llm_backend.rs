use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(PartialEq, Debug, Clone)]
pub enum PromptFormat {
    Mistral,
    Qwen,
}

pub struct ModelConfig {
    pub context_size: u32,
    pub model_path: String,
    pub prompt_format: PromptFormat,
    pub n_gpu_layers: u32,
}

pub struct GenerationConfig {
    pub max_tokens: usize,
    pub stop_strings: Vec<String>,
}

pub struct TokenInfo {
    #[allow(dead_code)]
    pub token_id: u32,
    pub token_str: String,
}

pub struct ContextInfo {
    #[allow(dead_code)]
    pub prompt_tokens: usize,
    #[allow(dead_code)]
    pub response_tokens: usize,
    pub total_tokens: usize,
    pub context_size: usize,
    pub usage_percent: u32,
}

/// Trait for LLM backend implementations
pub trait LLMBackend {
    /// Initialize the backend with model configuration
    fn initialize(config: ModelConfig) -> Result<Self> where Self: Sized;
    
    /// Generate a response to a conversation
    fn generate_response(
        &mut self,
        conversation: &[ChatMessage],
        config: GenerationConfig,
        on_token: Box<dyn FnMut(TokenInfo) -> bool>, // Returns true to continue, false to stop
    ) -> Result<String>;
    
    /// Get context usage information
    fn get_context_info(&self, conversation: &[ChatMessage], response: &str) -> Result<ContextInfo>;
    
    /// Get backend name for identification
    fn backend_name(&self) -> &'static str;
    
    /// Clear any cached state
    #[allow(dead_code)]
    fn clear_cache(&mut self) -> Result<()>;
}