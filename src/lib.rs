use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::State;

// Re-export the chat logic - use real implementation by default
#[cfg(not(feature = "mock"))]
pub mod chat;
#[cfg(not(feature = "mock"))]
use chat::{ChatConfig, ChatEngine, SamplerType};

// Use mock implementation only when explicitly enabled (for E2E tests)
#[cfg(feature = "mock")]
pub mod chat_mock;
#[cfg(feature = "mock")]
use chat_mock::{ChatConfig, ChatEngine, SamplerType};

// Application state
pub struct AppState {
    pub conversations: Arc<Mutex<HashMap<String, Vec<Message>>>>,
    pub chat_engine: Arc<Mutex<Option<ChatEngine>>>,
    pub sampler_config: Arc<Mutex<SamplerConfig>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            conversations: Arc::new(Mutex::new(HashMap::new())),
            chat_engine: Arc::new(Mutex::new(None)),
            sampler_config: Arc::new(Mutex::new(SamplerConfig::default())),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub role: String, // "user", "assistant", "system"
    pub content: String,
    pub timestamp: u64,
}

#[derive(Serialize, Deserialize)]
pub struct ChatRequest {
    pub message: String,
    pub conversation_id: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct ChatResponse {
    pub message: Message,
    pub conversation_id: String,
}

// Configuration types that match our existing constants
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SamplerConfig {
    pub sampler_type: String,
    pub temperature: f32,
    pub top_p: f32,
    pub top_k: u32,
    pub mirostat_tau: f32,
    pub mirostat_eta: f32,
    pub model_path: Option<String>,
    pub system_prompt: Option<String>,
    pub gpu_layers: Option<u32>, // Number of layers to offload to GPU
}

// Model management types
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelStatus {
    pub loaded: bool,
    pub model_path: Option<String>,
    pub last_used: Option<String>,
    pub memory_usage_mb: Option<u64>,
}

#[derive(Serialize, Deserialize)]
pub struct ModelLoadRequest {
    pub model_path: String,
}

#[derive(Serialize, Deserialize)]
pub struct ModelResponse {
    pub success: bool,
    pub message: String,
    pub status: Option<ModelStatus>,
}

impl Default for SamplerConfig {
    fn default() -> Self {
        // Get default system prompt from test.rs
        let default_system_prompt = r#"
You are a local cli AI tool with shell access on a computer, your goal is to understand what the user wants and help with tasks.
The current system is running on your detected OS
From that, you must automatically know what commands are available and how to format them

Rules of operation
- Don't ask the user to do tasks you can do
- You can freely manipulate files or folders for normal work.
- Try at least 10 times to do the tasks with a different approach before requesting more information to the user if you are stuck 
- Confirm only for risky changes (for example, deleting or overwriting many files, running privileged commands, installing software, or altering system paths).
- Before working with a file, verify that it exists first
- When looking for files: if not found in current directory, immediately use: find . -name "*filename*" -type f
- For file searches: use wildcards to match partial names across the entire project (e.g., find . -name "*alejandro*" -type f)
- IMPORTANT: Always put wildcards in quotes when using find command (e.g., "*.gguf" not *.gguf)
- NEVER search the entire filesystem with find / - use specific directories like . or ~/
- After finding file location, navigate and read the file from its actual path
- Always check subdirectories that seem relevant to the file you're looking for
- Always be thorough - execute search commands, don't just describe them
- Summarize the output briefly after execution and what you think about it.
- If a command fails, show the error and try a different approach - don't repeat the same failing command
- For web access, use curl, wget, or PowerShell's Invoke-WebRequest, with short timeouts and limited output.
- Keep responses concise, technical, and neutral.
- Try to run commands without moving from the current directory, don't use the 'cd' command
- Don't repeat the same commands over and over again

To run a command, use this exact format:
<COMMAND>command_here</COMMAND>
"#;

        Self {
            sampler_type: "Greedy".to_string(),
            temperature: 0.7,
            top_p: 0.95,
            top_k: 20,
            mirostat_tau: 5.0,
            mirostat_eta: 0.1,
            model_path: None, // No default model path - user must select one
            system_prompt: Some(default_system_prompt.trim().to_string()),
            gpu_layers: Some(32), // Default to 32 layers for RTX 4090
        }
    }
}

// Tauri commands (exposed to frontend)
pub async fn send_message(
    request: ChatRequest,
    state: State<'_, AppState>,
) -> Result<ChatResponse, String> {
    let conversation_id = request
        .conversation_id
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    // Create user message
    let user_message = Message {
        id: uuid::Uuid::new_v4().to_string(),
        role: "user".to_string(),
        content: request.message.clone(),
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    };

    // Add to conversation history
    {
        let mut conversations = state.conversations.lock().unwrap();
        let conversation = conversations
            .entry(conversation_id.clone())
            .or_default();
        conversation.push(user_message.clone());
    }

    // Get current config by cloning to avoid holding locks
    let current_config = {
        let config_guard = state.sampler_config.lock().unwrap();
        config_guard.clone()
    };

    // Create a chat engine for this request using the current config
    let chat_config = ChatConfig {
        sampler_type: SamplerType::from_string(&current_config.sampler_type),
        temperature: current_config.temperature,
        top_p: current_config.top_p,
        top_k: current_config.top_k,
        mirostat_tau: current_config.mirostat_tau,
        mirostat_eta: current_config.mirostat_eta,
        typical_p: 1.0,
        min_p: 0.0,
    };

    // Generate AI response using ChatEngine
    // Note: ChatEngine::new uses MODEL_PATH environment variable
    let ai_response_content = if let Some(_model_path) = &current_config.model_path {
        match ChatEngine::new(chat_config) {
            Ok(engine) => engine
                .generate_response(&request.message)
                .await
                .unwrap_or_else(|e| format!("Error generating response: {e}")),
            Err(e) => {
                // Clear invalid model path from config when model fails to load
                {
                    let mut config_guard = state.sampler_config.lock().unwrap();
                    config_guard.model_path = None;
                }
                format!(
                    "Model failed to load (path cleared): {e}. Please load a valid model."
                )
            }
        }
    } else {
        "No model loaded. Please load a model first.".to_string()
    };

    let ai_message = Message {
        id: uuid::Uuid::new_v4().to_string(),
        role: "assistant".to_string(),
        content: ai_response_content,
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    };

    // Add AI response to conversation
    {
        let mut conversations = state.conversations.lock().unwrap();
        let conversation = conversations.get_mut(&conversation_id).unwrap();
        conversation.push(ai_message.clone());
    }

    Ok(ChatResponse {
        message: ai_message,
        conversation_id,
    })
}

pub async fn get_conversations(
    state: State<'_, AppState>,
) -> Result<HashMap<String, Vec<Message>>, String> {
    let conversations = state.conversations.lock().unwrap();
    Ok(conversations.clone())
}

pub async fn get_conversation(
    conversation_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<Message>, String> {
    let conversations = state.conversations.lock().unwrap();
    Ok(conversations
        .get(&conversation_id)
        .cloned()
        .unwrap_or_default())
}

pub async fn get_sampler_config() -> Result<SamplerConfig, String> {
    // Return the default config for now
    // In a real app, this would read from a config file or database
    Ok(SamplerConfig::default())
}

pub async fn update_sampler_config(config: SamplerConfig) -> Result<(), String> {
    // Store the updated configuration
    // Note: This will require reinitializing the chat engine with new config
    println!("Updated sampler config: {config:?}");
    // In a real implementation, we'd save this to a file or database
    // and potentially restart the chat engine with the new configuration
    Ok(())
}

// Model management functions
pub async fn get_model_status(state: State<'_, AppState>) -> Result<ModelStatus, String> {
    // Check if the model is loaded
    let chat_engine = state.chat_engine.lock().unwrap();
    let config = state.sampler_config.lock().unwrap();

    let status = ModelStatus {
        loaded: chat_engine.is_some(),
        model_path: config.model_path.clone(),
        last_used: None, // Could track this if needed
        memory_usage_mb: if chat_engine.is_some() {
            Some(512)
        } else {
            None
        }, // Estimate
    };

    Ok(status)
}

pub async fn load_model(
    request: ModelLoadRequest,
    state: State<'_, AppState>,
) -> Result<ModelResponse, String> {
    // Update the model path in config
    {
        let mut config = state.sampler_config.lock().unwrap();
        config.model_path = Some(request.model_path.clone());
    }

    // Try to actually load the model to verify it works (real implementation)
    #[cfg(not(feature = "mock"))]
    {
        let config = ChatConfig::default();
        // Note: ChatEngine::new uses MODEL_PATH environment variable
        // For desktop app, set MODEL_PATH env var before loading
        std::env::set_var("MODEL_PATH", &request.model_path);
        match ChatEngine::new(config) {
            Ok(_) => {
                // Model loaded successfully
                let status = ModelStatus {
                    loaded: true,
                    model_path: Some(request.model_path.clone()),
                    last_used: None,
                    memory_usage_mb: Some(512), // Estimate
                };

                Ok(ModelResponse {
                    success: true,
                    message: format!("Model loaded successfully from {}", request.model_path),
                    status: Some(status),
                })
            }
            Err(e) => {
                // Failed to load model, clear the path
                let mut config = state.sampler_config.lock().unwrap();
                config.model_path = None;

                Ok(ModelResponse {
                    success: false,
                    message: format!("Failed to load model: {e}"),
                    status: None,
                })
            }
        }
    }

    // Mock implementation for E2E tests
    #[cfg(feature = "mock")]
    {
        let config = ChatConfig::default();
        match ChatEngine::new_with_model(config, &request.model_path) {
            Ok(_) => {
                let status = ModelStatus {
                    loaded: true,
                    model_path: Some(request.model_path.clone()),
                    last_used: None,
                    memory_usage_mb: Some(512),
                };

                Ok(ModelResponse {
                    success: true,
                    message: format!(
                        "Model loaded successfully from {} (mock mode)",
                        request.model_path
                    ),
                    status: Some(status),
                })
            }
            Err(e) => {
                // Mock validation failed (e.g., file doesn't exist)
                let mut config = state.sampler_config.lock().unwrap();
                config.model_path = None;

                Ok(ModelResponse {
                    success: false,
                    message: format!("Failed to load model: {}", e),
                    status: None,
                })
            }
        }
    }
}

pub async fn unload_model(state: State<'_, AppState>) -> Result<ModelResponse, String> {
    // Clear the chat engine
    {
        let mut chat_engine = state.chat_engine.lock().unwrap();
        *chat_engine = None;
    }

    let status = ModelStatus {
        loaded: false,
        model_path: None,
        last_used: None,
        memory_usage_mb: None,
    };

    Ok(ModelResponse {
        success: true,
        message: "Model unloaded successfully".to_string(),
        status: Some(status),
    })
}

// Model metadata types
#[derive(Serialize, Deserialize)]
pub struct ModelMetadata {
    pub name: String,
    pub architecture: String,
    pub parameters: String,
    pub quantization: String,
    pub file_size: String,
    pub context_length: String,
    pub file_path: String,
}

pub async fn get_model_metadata(model_path: String) -> Result<ModelMetadata, String> {
    use std::fs;

    // Get basic file info
    let file_metadata =
        fs::metadata(&model_path).map_err(|e| format!("Failed to read file metadata: {e}"))?;

    let file_size = file_metadata.len();
    let file_size_str = if file_size > 1024 * 1024 * 1024 {
        format!("{:.1} GB", file_size as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if file_size > 1024 * 1024 {
        format!("{:.1} MB", file_size as f64 / (1024.0 * 1024.0))
    } else {
        format!("{file_size} bytes")
    };

    let file_name = std::path::Path::new(&model_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Unknown")
        .to_string();

    // Try to read GGUF metadata from the file with extra safety
    let (architecture, parameters, quantization, context_length) =
        std::panic::catch_unwind(|| read_gguf_basic_metadata(&model_path))
            .unwrap_or_else(|_| Err("GGUF parsing panicked - corrupted file format".to_string()))
            .unwrap_or_else(|e| {
                println!(
                    "Failed to read GGUF metadata: {e}, falling back to filename parsing"
                );
                let (arch, params, quant) = parse_model_filename(&file_name);
                (arch, params, quant, "Unknown".to_string())
            });

    Ok(ModelMetadata {
        name: file_name,
        architecture,
        parameters,
        quantization,
        file_size: file_size_str,
        context_length,
        file_path: model_path,
    })
}

// GGUF metadata helper functions (using gguf_llms crate)
// Note: Shared utilities available in web::gguf_utils for web server code

fn read_gguf_basic_metadata(file_path: &str) -> Result<(String, String, String, String), String> {
    use gguf_llms::{GgufHeader, GgufReader, Value};
    use std::fs::File;
    use std::io::BufReader;

    let file = File::open(file_path).map_err(|e| format!("Failed to open file: {e}"))?;

    let mut reader = BufReader::new(file);

    let header = GgufHeader::parse(&mut reader)
        .map_err(|e| format!("Failed to parse GGUF header: {e}"))?;

    let metadata = GgufReader::read_metadata(&mut reader, header.n_kv)
        .map_err(|e| format!("Failed to read GGUF metadata: {e}"))?;

    // Helper to get metadata value as string
    let get_string = |key: &str| -> Option<String> {
        metadata.get(key).and_then(|v| match v {
            Value::String(s) => Some(s.clone()),
            Value::Uint32(n) => Some(n.to_string()),
            Value::Uint64(n) => Some(n.to_string()),
            Value::Int32(n) => Some(n.to_string()),
            Value::Int64(n) => Some(n.to_string()),
            _ => None,
        })
    };

    let architecture = get_string("general.architecture")
        .or_else(|| get_string("general.arch"))
        .unwrap_or_else(|| "Unknown".to_string());

    let parameters = get_string("general.parameter_count")
        .or_else(|| get_string("general.param_count"))
        .map(|p| format_parameter_count(&p))
        .unwrap_or_else(|| "Unknown".to_string());

    let quantization = get_string("general.quantization_version")
        .or_else(|| get_string("general.file_type"))
        .unwrap_or_else(|| "Unknown".to_string());

    let context_length = get_string(&format!("{architecture}.context_length"))
        .or_else(|| get_string("llama.context_length"))
        .or_else(|| get_string("context_length"))
        .unwrap_or_else(|| "Unknown".to_string());

    Ok((architecture, parameters, quantization, context_length))
}

fn format_parameter_count(param_str: &str) -> String {
    if let Ok(count) = param_str.parse::<u64>() {
        if count >= 1_000_000_000 {
            format!("{}B", count / 1_000_000_000)
        } else if count >= 1_000_000 {
            format!("{}M", count / 1_000_000)
        } else {
            count.to_string()
        }
    } else {
        param_str.to_string()
    }
}

fn parse_model_filename(filename: &str) -> (String, String, String) {
    let lower = filename.to_lowercase();

    // Extract architecture
    let architecture = if lower.contains("llama") {
        "LLaMA"
    } else if lower.contains("mistral") {
        "Mistral"
    } else if lower.contains("qwen") {
        "Qwen"
    } else if lower.contains("granite") {
        "Granite"
    } else if lower.contains("gemma") {
        "Gemma"
    } else {
        "Unknown"
    }
    .to_string();

    // Extract parameter count
    let parameters = ["70b", "34b", "13b", "8b", "7b", "3b", "1b"]
        .iter()
        .find(|p| lower.contains(*p))
        .map(|p| p.to_uppercase())
        .unwrap_or_else(|| "Unknown".to_string());

    // Extract quantization
    let quantization = [
        "q8_0", "q6_k", "q5_k_m", "q4_k_m", "q4_k_s", "q4_0", "q3_k_m", "q2_k",
    ]
    .iter()
    .find(|q| lower.contains(*q))
    .map(|q| q.to_uppercase())
    .unwrap_or_else(|| "Unknown".to_string());

    (architecture, parameters, quantization)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_metadata_extraction() {
        // Test with the small test file we have
        let test_path = "./assets/test-models/test.gguf";
        if std::path::Path::new(test_path).exists() {
            match get_model_metadata(test_path.to_string()).await {
                Ok(metadata) => {
                    println!("Test metadata extraction successful:");
                    println!("  Name: {}", metadata.name);
                    println!("  Architecture: {}", metadata.architecture);
                    println!("  Parameters: {}", metadata.parameters);
                    println!("  Quantization: {}", metadata.quantization);
                    println!("  File size: {}", metadata.file_size);
                    println!("  Context length: {}", metadata.context_length);
                }
                Err(e) => {
                    println!("Test metadata extraction failed: {e}");
                }
            }
        } else {
            println!("Test file not found, skipping metadata test");
        }
    }
}
