// Simple web server version of LLaMA Chat (without Tauri)
use std::net::SocketAddr;
use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use std::num::NonZeroU32;
use std::fs;
use std::io;
use serde::{Deserialize, Serialize};
use serde_json;
use gguf_llms::{GgufHeader, GgufReader, Value};
use std::io::BufReader;
use tokio::sync::mpsc;
use hyper::body::Bytes;
use std::sync::atomic::{AtomicU32, Ordering};

// HTTP server using hyper
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Method, Request, Response, Server, StatusCode};
use hyper::upgrade::Upgraded;
use tokio_tungstenite::tungstenite::protocol::Message as WsMessage;
use tokio_tungstenite::WebSocketStream;
use futures_util::{StreamExt, SinkExt};

// LLaMA integration

use llama_cpp_2::{
    context::params::LlamaContextParams,
    llama_backend::LlamaBackend,
    llama_batch::LlamaBatch,
    model::{params::LlamaModelParams, AddBos, LlamaModel, Special},
    sampling::LlamaSampler,
    // send_logs_to_tracing, LogOptions,
};

// Global counter for active WebSocket connections
static ACTIVE_WS_CONNECTIONS: AtomicU32 = AtomicU32::new(0);

// Configuration structure
#[derive(Deserialize, Serialize, Clone)]
struct SamplerConfig {
    sampler_type: String,
    temperature: f64,
    top_p: f64,
    top_k: u32,
    mirostat_tau: f64,
    mirostat_eta: f64,
    model_path: Option<String>,
    system_prompt: Option<String>,
    context_size: Option<u32>,
    stop_tokens: Option<Vec<String>>,
    #[serde(default)]
    model_history: Vec<String>,
}

// Token data with metadata for streaming
#[derive(Serialize, Clone)]
struct TokenData {
    token: String,
    tokens_used: i32,
    max_tokens: i32,
}

// Common stop tokens for different model providers
fn get_common_stop_tokens() -> Vec<String> {
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
            sampler_type: "Greedy".to_string(),
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

// Request/Response structures
#[derive(Deserialize)]
struct ChatRequest {
    message: String,
    conversation_id: Option<String>,
}

#[derive(Serialize)]
struct ChatResponse {
    message: ChatMessage,
    conversation_id: String,
    tokens_used: Option<i32>,
    max_tokens: Option<i32>,
}

#[derive(Serialize)]
struct ChatMessage {
    id: String,
    role: String,
    content: String,
    timestamp: u64,
}

#[derive(Serialize)]
struct ConversationFile {
    name: String,
    display_name: String,
    timestamp: String,
}

#[derive(Serialize)]
struct ConversationsResponse {
    conversations: Vec<ConversationFile>,
}

#[derive(Serialize)]
struct ConversationContentResponse {
    content: String,
    messages: Vec<ChatMessage>,
}

#[derive(Serialize)]
struct FileItem {
    name: String,
    path: String,
    is_directory: bool,
    size: Option<u64>,
}

#[derive(Serialize)]
struct BrowseFilesResponse {
    files: Vec<FileItem>,
    current_path: String,
    parent_path: Option<String>,
}

#[derive(Serialize)]
struct ModelStatus {
    loaded: bool,
    model_path: Option<String>,
    last_used: Option<String>,
    memory_usage_mb: Option<u64>,
}

#[derive(Deserialize)]
struct ModelLoadRequest {
    model_path: String,
}

#[derive(Serialize)]
struct ModelResponse {
    success: bool,
    message: String,
    status: Option<ModelStatus>,
}

// Shared state for LLaMA

struct LlamaState {
    backend: LlamaBackend,
    model: Option<LlamaModel>,
    current_model_path: Option<String>,
    model_context_length: Option<u32>,
    chat_template_type: Option<String>, // Store detected template type
    last_used: std::time::SystemTime,
}


type SharedLlamaState = Arc<Mutex<Option<LlamaState>>>;

type SharedConversationLogger = Arc<Mutex<ConversationLogger>>;

// Helper function to load configuration
fn load_config() -> SamplerConfig {
    let config_path = "assets/config.json";
    match fs::read_to_string(config_path) {
        Ok(content) => {
            match serde_json::from_str::<SamplerConfig>(&content) {
                Ok(config) => config,
                Err(e) => {
                    eprintln!("Failed to parse config file: {}, using defaults", e);
                    SamplerConfig::default()
                }
            }
        }
        Err(_) => {
            // Config file doesn't exist, use defaults
            SamplerConfig::default()
        }
    }
}

// Helper function to add a model path to history
fn add_to_model_history(model_path: &str) {
    let config_path = "assets/config.json";

    // Load current config
    let mut config = load_config();

    // Remove the path if it already exists (to move it to the front)
    config.model_history.retain(|p| p != model_path);

    // Add to the front of the list
    config.model_history.insert(0, model_path.to_string());

    // Keep only the last 10 paths
    if config.model_history.len() > 10 {
        config.model_history.truncate(10);
    }

    // Save the updated config
    let _ = fs::create_dir_all("assets");
    if let Ok(json) = serde_json::to_string_pretty(&config) {
        let _ = fs::write(config_path, json);
    }
}

// Helper function to parse command with proper quote handling
fn parse_command_with_quotes(cmd: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current_part = String::new();
    let mut in_quotes = false;
    let mut chars = cmd.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
                // Don't include the quote character in the output
            }
            ' ' if !in_quotes => {
                if !current_part.is_empty() {
                    parts.push(current_part.clone());
                    current_part.clear();
                }
            }
            _ => {
                current_part.push(ch);
            }
        }
    }

    if !current_part.is_empty() {
        parts.push(current_part);
    }

    parts
}

// Helper function to execute system commands
fn execute_command(cmd: &str) -> String {
    use std::process::Command;
    use std::env;

    // Parse command with proper quote handling
    let parts = parse_command_with_quotes(cmd.trim());
    if parts.is_empty() {
        return "Error: Empty command".to_string();
    }

    let command_name = &parts[0];

    // Basic command validation - reject obviously invalid commands
    if command_name.len() < 2 || command_name.contains("/") && !command_name.starts_with("/") {
        return format!("Error: Invalid command format: {}", command_name);
    }

    // Prevent dangerous filesystem-wide searches
    if command_name == "find" && parts.len() > 1 {
        let search_path = &parts[1];
        if search_path == "/" || search_path == "/usr" || search_path == "/System" {
            return format!("Error: Filesystem-wide searches are not allowed for performance and security reasons. Try searching in specific directories like current directory '.'");
        }
    }

    // Special handling for cd command - actually change the process working directory
    if command_name == "cd" {
        let target_dir = if parts.len() > 1 {
            &parts[1]
        } else {
            return "Error: cd command requires a directory argument".to_string();
        };

        match env::set_current_dir(target_dir) {
            Ok(_) => {
                if let Ok(new_dir) = env::current_dir() {
                    format!("Successfully changed directory to: {}", new_dir.display())
                } else {
                    "Directory changed successfully".to_string()
                }
            }
            Err(e) => {
                format!("Error: Failed to change directory: {}", e)
            }
        }
    } else {
        // Normal command execution for non-cd commands
        let mut command = Command::new(&parts[0]);
        if parts.len() > 1 {
            command.args(&parts[1..]);
        }

        match command.output() {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);

                // Handle commands that succeed silently
                if output.status.success() && stdout.is_empty() && stderr.is_empty() {
                    match command_name.as_str() {
                        "find" => "No files found matching the search criteria".to_string(),
                        "mkdir" => "Directory created successfully".to_string(),
                        "touch" => "File created successfully".to_string(),
                        "rm" | "rmdir" => "File/directory removed successfully".to_string(),
                        "mv" | "cp" => "File operation completed successfully".to_string(),
                        "chmod" => "Permissions changed successfully".to_string(),
                        _ => {
                            if parts.len() > 1 {
                                format!("Command '{}' executed successfully", parts.join(" "))
                            } else {
                                format!("Command '{}' executed successfully", command_name)
                            }
                        }
                    }
                } else if !stderr.is_empty() {
                    format!("{}\nError: {}", stdout, stderr)
                } else {
                    stdout.to_string()
                }
            }
            Err(e) => {
                format!("Failed to execute command: {}", e)
            }
        }
    }
}

// Helper function to get model status

fn get_model_status(llama_state: &SharedLlamaState) -> ModelStatus {
    match llama_state.lock() {
        Ok(state_guard) => {
            match state_guard.as_ref() {
                Some(state) => {
                    let loaded = state.model.is_some();
                    let model_path = state.current_model_path.clone();
                    let last_used = state.last_used
                        .duration_since(std::time::UNIX_EPOCH)
                        .ok()
                        .map(|d| d.as_secs().to_string());
                    
                    ModelStatus {
                        loaded,
                        model_path,
                        last_used,
                        memory_usage_mb: if loaded { Some(512) } else { None }, // Rough estimate
                    }
                }
                None => ModelStatus {
                    loaded: false,
                    model_path: None,
                    last_used: None,
                    memory_usage_mb: None,
                }
            }
        }
        Err(_) => ModelStatus {
            loaded: false,
            model_path: None,
            last_used: None,
            memory_usage_mb: None,
        }
    }
}

// Helper function to calculate optimal GPU layers based on available VRAM

fn calculate_optimal_gpu_layers(model_path: &str) -> u32 {
    use std::fs;

    // Get model file size to estimate memory requirements
    let model_size_bytes = match fs::metadata(model_path) {
        Ok(metadata) => metadata.len(),
        Err(_) => {
            println!("[GPU] Could not read model file size, defaulting to 32 layers");
            return 32;
        }
    };

    let model_size_gb = model_size_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
    println!("[GPU] Model file size: {:.2} GB", model_size_gb);

    // Try to get available GPU VRAM
    // For NVIDIA GPUs, we can estimate based on typical model requirements
    // A rough heuristic:
    // - Small models (< 5GB): Use all GPU layers (typically ~40 layers)
    // - Medium models (5-15GB): Use proportional layers
    // - Large models (> 15GB): May need CPU offload

    // Estimate based on RTX 4090 with ~24GB VRAM
    // Reserve ~2GB for system/context, leaving ~22GB for model
    let available_vram_gb = 22.0;

    println!("[GPU] Estimated available VRAM: {:.2} GB", available_vram_gb);

    // Calculate what percentage of the model fits in VRAM
    let vram_ratio = (available_vram_gb / model_size_gb).min(1.0);

    // Estimate typical layer count based on model size
    // Small models (~7B params, ~4-8GB): ~32-40 layers
    // Medium models (~13B params, ~8-15GB): ~40-50 layers
    // Large models (~30B+ params, >15GB): ~50-80 layers
    let estimated_total_layers = if model_size_gb < 8.0 {
        36
    } else if model_size_gb < 15.0 {
        45
    } else if model_size_gb < 25.0 {
        60
    } else {
        80
    };

    let optimal_layers = (estimated_total_layers as f64 * vram_ratio).floor() as u32;

    println!("[GPU] Estimated total layers: {}", estimated_total_layers);
    println!("[GPU] VRAM utilization ratio: {:.1}%", vram_ratio * 100.0);
    println!("[GPU] Optimal GPU layers: {} ({}% of model)",
             optimal_layers,
             (optimal_layers as f64 / estimated_total_layers as f64 * 100.0) as u32);

    // Ensure at least 1 layer on GPU if model is small enough
    optimal_layers.max(if vram_ratio > 0.1 { 1 } else { 0 })
}

// Helper function to load a model

async fn load_model(llama_state: SharedLlamaState, model_path: &str) -> Result<(), String> {
    println!("[DEBUG] load_model called with path: {}", model_path);

    // Handle poisoned mutex by recovering from panic
    let mut state_guard = llama_state.lock().unwrap_or_else(|poisoned| {
        println!("[DEBUG] Mutex was poisoned, recovering...");
        poisoned.into_inner()
    });

    // Initialize backend if needed
    if state_guard.is_none() {
        let backend = LlamaBackend::init().map_err(|e| format!("Failed to init backend: {}", e))?;
        *state_guard = Some(LlamaState {
            backend,
            model: None,
            current_model_path: None,
            model_context_length: None,
            chat_template_type: None,
            last_used: std::time::SystemTime::now(),
        });
    }

    let state = state_guard.as_mut().unwrap();

    // Check if model is already loaded
    if let Some(ref current_path) = state.current_model_path {
        if current_path == model_path && state.model.is_some() {
            state.last_used = std::time::SystemTime::now();
            return Ok(()); // Model already loaded
        }
    }

    // Unload current model if any
    state.model = None;
    state.current_model_path = None;

    // Calculate optimal GPU layers
    let optimal_gpu_layers = calculate_optimal_gpu_layers(model_path);

    // Load new model with calculated GPU acceleration
    let model_params = LlamaModelParams::default()
        .with_n_gpu_layers(optimal_gpu_layers);

    println!("Loading model from: {}", model_path);
    println!("GPU layers configured: {} layers will be offloaded to GPU", optimal_gpu_layers);

    let model = LlamaModel::load_from_file(&state.backend, model_path, &model_params)
        .map_err(|e| format!("Failed to load model: {}", e))?;

    println!("Model loaded successfully!");

    // Read model's context length, token IDs, and chat template from GGUF metadata
    let (model_context_length, bos_token_id, eos_token_id, chat_template_type) = if let Ok(file) = fs::File::open(model_path) {
        let mut reader = BufReader::new(file);
        if let Ok(header) = GgufHeader::parse(&mut reader) {
            if let Ok(metadata) = GgufReader::read_metadata(&mut reader, header.n_kv) {
                let ctx_len = metadata.get("llama.context_length")
                    .and_then(|v| match v {
                        Value::Uint32(n) => Some(*n),
                        Value::Uint64(n) => Some(*n as u32),
                        _ => None,
                    });

                let bos_id = metadata.get("tokenizer.ggml.bos_token_id")
                    .and_then(|v| match v {
                        Value::Uint32(n) => Some(*n as i32),
                        Value::Int32(n) => Some(*n),
                        _ => None,
                    });

                let eos_id = metadata.get("tokenizer.ggml.eos_token_id")
                    .and_then(|v| match v {
                        Value::Uint32(n) => Some(*n as i32),
                        Value::Int32(n) => Some(*n),
                        _ => None,
                    });

                // Detect chat template type
                let template_type = metadata.get("tokenizer.chat_template")
                    .and_then(|v| match v {
                        Value::String(s) => {
                            // Detect template type based on template content
                            if s.contains("<|im_start|>") && s.contains("<|im_end|>") {
                                Some("ChatML".to_string()) // Qwen, OpenAI format
                            } else if s.contains("[INST]") && s.contains("[/INST]") {
                                Some("Mistral".to_string()) // Mistral format
                            } else if s.contains("<|start_header_id|>") {
                                Some("Llama3".to_string()) // Llama 3 format
                            } else {
                                Some("Generic".to_string()) // Fallback
                            }
                        }
                        _ => None,
                    });

                (ctx_len, bos_id, eos_id, template_type)
            } else {
                (None, None, None, None)
            }
        } else {
            (None, None, None, None)
        }
    } else {
        (None, None, None, None)
    };

    if let Some(ctx_len) = model_context_length {
        println!("Model context length from GGUF: {}", ctx_len);
    }
    if let Some(bos) = bos_token_id {
        println!("Model BOS token ID from GGUF: {}", bos);
    }
    if let Some(eos) = eos_token_id {
        println!("Model EOS token ID from GGUF: {}", eos);

        // Validate against what the model reports
        let model_eos = model.token_eos().0; // Extract underlying i32 from LlamaToken
        if eos != model_eos {
            println!("WARNING: GGUF EOS token ({}) doesn't match model.token_eos() ({})", eos, model_eos);
        } else {
            println!("✓ EOS token validation passed: GGUF and model agree on token {}", eos);
        }
    }

    if let Some(ref template) = chat_template_type {
        println!("Detected chat template type: {}", template);
    } else {
        println!("No chat template detected, using Mistral format as default");
    }

    state.model = Some(model);
    state.current_model_path = Some(model_path.to_string());
    state.model_context_length = model_context_length;
    state.chat_template_type = chat_template_type;
    state.last_used = std::time::SystemTime::now();

    Ok(())
}

// Helper function to unload the current model

async fn unload_model(llama_state: SharedLlamaState) -> Result<(), String> {
    let mut state_guard = llama_state.lock().map_err(|_| "Failed to lock LLaMA state")?;
    
    if let Some(state) = state_guard.as_mut() {
        state.model = None;
        state.current_model_path = None;
    }
    
    Ok(())
}

// Conversation logging
struct ConversationLogger {
    file_path: String,
    content: String,
}

impl ConversationLogger {
    fn new(system_prompt: Option<&str>) -> io::Result<Self> {
        // Create assets/conversations directory if it doesn't exist
        let conversations_dir = "assets/conversations";
        fs::create_dir_all(conversations_dir)?;

        // Generate timestamp-based filename with YYYY-MM-DD-HH-mm-ss-SSS format
        let now = std::time::SystemTime::now();
        let since_epoch = now
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        // Convert to a more readable format
        let secs = since_epoch.as_secs();
        let millis = since_epoch.subsec_millis();

        // Simple conversion (this won't be perfect timezone-wise, but it's readable)
        let days_since_epoch = secs / 86400;
        let remaining_secs = secs % 86400;
        let hours = remaining_secs / 3600;
        let remaining_secs = remaining_secs % 3600;
        let minutes = remaining_secs / 60;
        let seconds = remaining_secs % 60;

        // Approximate date calculation (starting from 1970-01-01)
        let year = 1970 + (days_since_epoch / 365);
        let day_of_year = days_since_epoch % 365;
        let month = std::cmp::min(12, (day_of_year / 30) + 1);
        let day = (day_of_year % 30) + 1;

        let timestamp = format!(
            "{:04}-{:02}-{:02}-{:02}-{:02}-{:02}-{:03}",
            year, month, day, hours, minutes, seconds, millis
        );

        let file_path = format!("{}/chat_{}.txt", conversations_dir, timestamp);

        let mut logger = ConversationLogger {
            file_path,
            content: String::new(),
        };

        // Only log system prompt if one is explicitly provided
        // If None, the model's chat template will use its built-in default
        if let Some(prompt) = system_prompt {
            logger.log_message("SYSTEM", prompt);
        }

        Ok(logger)
    }

    fn from_existing(conversation_id: &str) -> io::Result<Self> {
        // Load existing conversation file
        let conversations_dir = "assets/conversations";

        // Handle .txt extension if already present
        let file_path = if conversation_id.ends_with(".txt") {
            format!("{}/{}", conversations_dir, conversation_id)
        } else {
            format!("{}/{}.txt", conversations_dir, conversation_id)
        };

        // Read existing content
        let content = fs::read_to_string(&file_path)?;

        Ok(ConversationLogger {
            file_path,
            content,
        })
    }

    fn log_message(&mut self, role: &str, message: &str) {
        let log_entry = format!("{}:\n{}\n\n", role, message);
        self.content.push_str(&log_entry);

        // Write immediately to file
        if let Err(e) = fs::write(&self.file_path, &self.content) {
            eprintln!("Failed to write conversation log: {}", e);
        }
    }

    fn log_token(&mut self, token: &str) {
        // Append token to the last assistant message in content
        self.content.push_str(token);

        // Write to file immediately so file watcher can update UI in real-time
        // This is now fast enough since we're not blocking on WebSocket
        if let Err(e) = fs::write(&self.file_path, &self.content) {
            eprintln!("Failed to write conversation log: {}", e);
        }
    }
    
    fn finish_assistant_message(&mut self) {
        // Add proper newlines after assistant message completion
        self.content.push_str("\n\n");

        // Write immediately to file
        if let Err(e) = fs::write(&self.file_path, &self.content) {
            eprintln!("Failed to write conversation log: {}", e);
        }
    }

    fn get_conversation_id(&self) -> String {
        // Extract filename from path (e.g., "assets/conversations/chat_2025-01-15-10-30-45-123.txt" -> "chat_2025-01-15-10-30-45-123.txt")
        std::path::Path::new(&self.file_path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("")
            .to_string()
    }

    fn log_command_execution(&mut self, command: &str, output: &str) {
        let log_entry = format!("[COMMAND: {}]\n{}\n\n", command, output);
        self.content.push_str(&log_entry);

        // Write immediately to file
        if let Err(e) = fs::write(&self.file_path, &self.content) {
            eprintln!("Failed to write conversation log: {}", e);
        }
    }

    fn get_full_conversation(&self) -> String {
        // Return the complete conversation content from memory
        self.content.clone()
    }

    fn load_conversation_from_file(&self) -> io::Result<String> {
        // Read the conversation directly from file (source of truth)
        fs::read_to_string(&self.file_path)
    }
}

fn parse_conversation_to_messages(conversation: &str) -> Vec<ChatMessage> {
    let mut messages = Vec::new();
    let mut current_role = "";
    let mut current_content = String::new();
    
    for line in conversation.lines() {
        if line.ends_with(":")
            && (line.starts_with("SYSTEM:")
                || line.starts_with("USER:")
                || line.starts_with("ASSISTANT:"))
        {
            // Save previous message if it exists
            if !current_role.is_empty() && !current_content.trim().is_empty() {
                let role = match current_role {
                    "SYSTEM" => "system",
                    "USER" => "user", 
                    "ASSISTANT" => "assistant",
                    _ => "user",
                };
                
                // Skip system messages in the UI
                if role != "system" {
                    messages.push(ChatMessage {
                        id: uuid::Uuid::new_v4().to_string(),
                        role: role.to_string(),
                        content: current_content.trim().to_string(),
                        timestamp: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs(),
                    });
                }
            }
            
            // Start new message
            current_role = line.trim_end_matches(":");
            current_content.clear();
        } else if !line.starts_with("[COMMAND:") && !line.trim().is_empty() {
            // Skip command execution logs, add content
            current_content.push_str(line);
            current_content.push('\n');
        }
    }
    
    // Add the final message
    if !current_role.is_empty() && !current_content.trim().is_empty() {
        let role = match current_role {
            "SYSTEM" => "system",
            "USER" => "user",
            "ASSISTANT" => "assistant", 
            _ => "user",
        };
        
        // Skip system messages in the UI
        if role != "system" {
            messages.push(ChatMessage {
                id: uuid::Uuid::new_v4().to_string(),
                role: role.to_string(),
                content: current_content.trim().to_string(),
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            });
        }
    }
    
    messages
}

#[allow(dead_code)]
fn get_available_tools_json() -> String {
    // Detect OS and provide appropriate command examples
    let os_name = std::env::consts::OS;
    let (description, example_commands) = match os_name {
        "windows" => (
            "Execute shell commands on Windows. Use 'dir' to list files, 'type' to read files, 'cd' to change directory, and other Windows cmd.exe commands.",
            "dir E:\\repo, type file.txt, cd C:\\Users"
        ),
        "linux" => (
            "Execute shell commands on Linux. Use 'ls' to list files, 'cat' to read files, 'cd' to change directory, and other bash commands.",
            "ls /home, cat file.txt, pwd"
        ),
        "macos" => (
            "Execute shell commands on macOS. Use 'ls' to list files, 'cat' to read files, 'cd' to change directory, and other bash commands.",
            "ls /Users, cat file.txt, pwd"
        ),
        _ => (
            "Execute shell commands on the system. Use this to interact with the filesystem, run programs, and check system information.",
            "Use appropriate commands for your operating system"
        )
    };

    serde_json::json!([
        {
            "type": "function",
            "function": {
                "name": "bash",
                "description": format!("{} OS: {}", description, os_name),
                "parameters": {
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": format!("The shell command to execute. Examples: {}", example_commands)
                        }
                    },
                    "required": ["command"]
                }
            }
        }
    ]).to_string()
}

fn apply_model_chat_template(conversation: &str, template_type: Option<&str>) -> Result<String, String> {
    // Parse conversation into messages
    let mut system_message: Option<String> = None;
    let mut user_messages = Vec::new();
    let mut assistant_messages = Vec::new();
    let mut current_role = "";
    let mut current_content = String::new();

    for line in conversation.lines() {
        if line.ends_with(":")
            && (line.starts_with("SYSTEM:")
                || line.starts_with("USER:")
                || line.starts_with("ASSISTANT:"))
        {
            // Save previous role's content
            if !current_role.is_empty() && !current_content.trim().is_empty() {
                match current_role {
                    "SYSTEM" => system_message = Some(current_content.trim().to_string()),
                    "USER" => user_messages.push(current_content.trim().to_string()),
                    "ASSISTANT" => assistant_messages.push(current_content.trim().to_string()),
                    _ => {}
                }
            }

            // Start new role
            current_role = line.trim_end_matches(":");
            current_content.clear();
        } else if !line.starts_with("[COMMAND:") {
            // Skip command execution logs, add content
            if !line.trim().is_empty() {
                current_content.push_str(line);
                current_content.push('\n');
            }
        }
    }

    // Add the final role content
    if !current_role.is_empty() && !current_content.trim().is_empty() {
        match current_role {
            "SYSTEM" => system_message = Some(current_content.trim().to_string()),
            "USER" => user_messages.push(current_content.trim().to_string()),
            "ASSISTANT" => assistant_messages.push(current_content.trim().to_string()),
            _ => {}
        }
    }

    // Construct prompt based on detected template type
    let prompt = match template_type {
        Some("ChatML") => {
            // Qwen/ChatML format: <|im_start|>role\ncontent<|im_end|>
            let mut p = String::new();

            // Add system message
            if let Some(sys_msg) = system_message {
                p.push_str("<|im_start|>system\n");
                p.push_str(&sys_msg);
                p.push_str("<|im_end|>\n");
            }

            // Add conversation history
            let turn_count = user_messages.len().max(assistant_messages.len());
            for i in 0..turn_count {
                if i < user_messages.len() {
                    p.push_str("<|im_start|>user\n");
                    p.push_str(&user_messages[i]);
                    p.push_str("<|im_end|>\n");
                }
                if i < assistant_messages.len() {
                    p.push_str("<|im_start|>assistant\n");
                    p.push_str(&assistant_messages[i]);
                    p.push_str("<|im_end|>\n");
                }
            }

            // Add generation prompt
            p.push_str("<|im_start|>assistant\n");

            p
        }
        Some("Mistral") | None => {
            // Mistral format: <s>[INST] user [/INST] assistant </s>
            let mut p = String::new();
            p.push_str("<s>");

            // Add system prompt if present
            if let Some(sys_msg) = system_message {
                p.push_str("[SYSTEM_PROMPT]");
                p.push_str(&sys_msg);
                p.push_str("[/SYSTEM_PROMPT]");
            }

            // Add conversation history
            let turn_count = user_messages.len().max(assistant_messages.len());
            for i in 0..turn_count {
                if i < user_messages.len() {
                    p.push_str("[INST]");
                    p.push_str(&user_messages[i]);
                    p.push_str("[/INST]");
                }
                if i < assistant_messages.len() {
                    p.push_str(&assistant_messages[i]);
                    p.push_str("</s>");
                }
            }

            p
        }
        Some("Llama3") => {
            // Llama 3 format
            let mut p = String::new();
            p.push_str("<|begin_of_text|>");

            if let Some(sys_msg) = system_message {
                p.push_str("<|start_header_id|>system<|end_header_id|>\n\n");
                p.push_str(&sys_msg);
                p.push_str("<|eot_id|>");
            }

            let turn_count = user_messages.len().max(assistant_messages.len());
            for i in 0..turn_count {
                if i < user_messages.len() {
                    p.push_str("<|start_header_id|>user<|end_header_id|>\n\n");
                    p.push_str(&user_messages[i]);
                    p.push_str("<|eot_id|>");
                }
                if i < assistant_messages.len() {
                    p.push_str("<|start_header_id|>assistant<|end_header_id|>\n\n");
                    p.push_str(&assistant_messages[i]);
                    p.push_str("<|eot_id|>");
                }
            }

            p.push_str("<|start_header_id|>assistant<|end_header_id|>\n\n");

            p
        }
        Some(_) => {
            // Generic fallback - use ChatML-style
            let mut p = String::new();

            if let Some(sys_msg) = system_message {
                p.push_str("System: ");
                p.push_str(&sys_msg);
                p.push_str("\n\n");
            }

            let turn_count = user_messages.len().max(assistant_messages.len());
            for i in 0..turn_count {
                if i < user_messages.len() {
                    p.push_str("User: ");
                    p.push_str(&user_messages[i]);
                    p.push_str("\n\n");
                }
                if i < assistant_messages.len() {
                    p.push_str("Assistant: ");
                    p.push_str(&assistant_messages[i]);
                    p.push_str("\n\n");
                }
            }

            p.push_str("Assistant: ");

            p
        }
    };

    // Debug: Print first 1000 chars of prompt
    eprintln!("\n[DEBUG] Template type: {:?}", template_type);
    eprintln!("[DEBUG] Constructed prompt (first 1000 chars):");
    eprintln!("{}", &prompt.chars().take(1000).collect::<String>());

    Ok(prompt)
}

// Constants for LLaMA configuration

const CONTEXT_SIZE: u32 = 32768;

const MODEL_PATH: &str = "/app/models/lmstudio-community/granite-4.0-h-tiny-GGUF/granite-4.0-h-tiny-Q4_K_M.gguf";


async fn generate_llama_response(
    user_message: &str,
    llama_state: SharedLlamaState,
    conversation_logger: SharedConversationLogger,
    token_sender: Option<mpsc::UnboundedSender<TokenData>>
) -> Result<(String, i32, i32), String> {
    // Log user message to conversation file
    {
        let mut logger = conversation_logger.lock().map_err(|_| "Failed to lock conversation logger")?;
        logger.log_message("USER", user_message);
    }
    
    // Load configuration to get model path and context size
    let config = load_config();
    let model_path = config.model_path.as_deref().unwrap_or(MODEL_PATH);
    let stop_tokens = config.stop_tokens.unwrap_or_else(get_common_stop_tokens);
    
    // Ensure model is loaded
    load_model(llama_state.clone(), model_path).await?;

    // Now use the shared state for generation
    let state_guard = llama_state.lock().map_err(|_| "Failed to lock LLaMA state")?;
    let state = state_guard.as_ref().ok_or("LLaMA state not initialized")?;
    let model = state.model.as_ref().ok_or("No model loaded")?;

    // Get context size: prefer user config, fallback to model's context_length, then default
    let context_size = config.context_size
        .or(state.model_context_length)
        .unwrap_or(CONTEXT_SIZE);

    println!("Using context size: {} (user config: {:?}, model max: {:?})",
        context_size, config.context_size, state.model_context_length);
    
    // Create sampler
    let mut sampler = LlamaSampler::greedy();
    
    // Read conversation history from file and create chat prompt
    let conversation_content = {
        let logger = conversation_logger.lock().map_err(|_| "Failed to lock conversation logger")?;
        logger.load_conversation_from_file().unwrap_or_else(|_| logger.get_full_conversation())
    };

    // Convert conversation to chat format using model's chat template
    let template_type = state.chat_template_type.clone();
    let prompt = apply_model_chat_template(&conversation_content, template_type.as_deref())?;
    
    // Tokenize
    let tokens = model
        .str_to_token(&prompt, AddBos::Never)
        .map_err(|e| format!("Tokenization failed: {}", e))?;
    
    // Create context with configured size
    let n_ctx = NonZeroU32::new(context_size).unwrap();
    let ctx_params = LlamaContextParams::default().with_n_ctx(Some(n_ctx));
    let mut context = model
        .new_context(&state.backend, ctx_params)
        .map_err(|e| format!("Context creation failed: {}", e))?;
    
    // Prepare batch
    let batch_size = std::cmp::min(tokens.len() + 512, 2048);
    let mut batch = LlamaBatch::new(batch_size, 1);
    for (i, &token) in tokens.iter().enumerate() {
        let is_last = i == tokens.len() - 1;
        batch
            .add(token, i as i32, &[0], is_last)
            .map_err(|e| format!("Batch add failed: {}", e))?;
    }
    
    // Process initial tokens
    context
        .decode(&mut batch)
        .map_err(|e| format!("Initial decode failed: {}", e))?;
    
    // Start assistant message in conversation log
    {
        let mut logger = conversation_logger.lock().map_err(|_| "Failed to lock conversation logger")?;
        logger.log_message("ASSISTANT", "");
    }

    // Generate response
    let mut response = String::new();
    let mut token_pos = tokens.len() as i32;
    let mut total_tokens_generated = 0;

    // Calculate max tokens based on remaining context space
    // Leave some buffer for safety (e.g., 128 tokens)
    let remaining_context = (context_size as i32) - token_pos - 128;
    let max_total_tokens = remaining_context.max(512); // Ensure at least 512 tokens if possible

    println!("Context size: {}, Prompt tokens: {}, Max tokens to generate: {}",
             context_size, token_pos, max_total_tokens);

    // Outer loop to handle command execution and continuation
    loop {
        let mut command_executed = false;
        let mut hit_stop_condition = false; // Track if we hit EOS or stop token

        // Inner loop for token generation
        let tokens_to_generate = std::cmp::min(2048, max_total_tokens - total_tokens_generated);

        println!("[DEBUG] Starting generation cycle: tokens_to_generate={}, total_tokens_generated={}", tokens_to_generate, total_tokens_generated);

        for _i in 0..tokens_to_generate { // Limit response length per cycle
        // Sample next token
        let next_token = sampler.sample(&context, -1);

        // Check for end-of-sequence token
        if next_token == model.token_eos() {
            println!("Debug: Stopping generation - EOS token detected (token ID: {})", next_token);
            hit_stop_condition = true;
            break;
        }

        // IMPORTANT: Add token to batch and decode FIRST, before string conversion
        // This ensures the model progresses even if we can't display the token
        batch.clear();
        batch
            .add(next_token, token_pos, &[0], true)
            .map_err(|e| format!("Batch add failed: {}", e))?;

        context
            .decode(&mut batch)
            .map_err(|e| format!("Decode failed: {}", e))?;

        token_pos += 1;
        total_tokens_generated += 1;

        // Now try to convert to string for display - if this fails, we skip display but keep going
        let token_str = match model.token_to_str(next_token, Special::Tokenize) {
            Ok(s) => s,
            Err(e) => {
                // Log UTF-8 error but continue generation
                // Token is already processed in context, just can't display it
                println!("[WARN] Token {} can't be displayed as UTF-8: {}. Continuing generation.", next_token, e);
                continue; // Skip display but token is already in context
            }
        };

        // Debug: log suspicious tokens
        if token_str.contains('<') || token_str.contains('>') || token_str.contains('|') {
            println!("Debug: Found special character in token: '{}'", token_str);
        }

        // IMPORTANT: Check for stop sequences BEFORE adding the token to the response
        // This prevents the stop sequences from appearing in the final output
        let test_response = format!("{}{}", response, token_str);

        // Check for any configured stop tokens
        let mut should_stop = false;
        let mut partial_to_remove = 0;

        // Check if we're inside a COMMAND block
        let in_command_block = response.contains("<COMMAND>") && !response.contains("</COMMAND>");

        for stop_token in &stop_tokens {
            // Check if the test response contains the complete stop token
            if test_response.contains(stop_token) {
                println!("Debug: Stopping generation due to stop token detected: '{}'", stop_token);

                // Special case: for </COMMAND>, we want to include it in the response
                if stop_token == "</COMMAND>" {
                    // Add the current token to complete the </COMMAND> tag
                    response.push_str(&token_str);

                    // Log token to conversation file
                    {
                        let mut logger = conversation_logger.lock().map_err(|_| "Failed to lock conversation logger")?;
                        logger.log_token(&token_str);
                    }
                }

                should_stop = true;
                break;
            }

            // Special handling when inside a COMMAND block
            // Don't stop on partial matches of stop tokens that start with "</" while generating a command
            if in_command_block && (stop_token.starts_with("</") || stop_token.starts_with("[/")) {
                continue; // Skip these stop tokens while inside COMMAND block
            }

            // Special handling for </COMMAND> - only stop on complete match, no partial matching
            // This allows the full command to be generated before stopping
            if stop_token == "</COMMAND>" {
                continue; // Skip partial matching for </COMMAND>
            }

            // Check if we're at the beginning of a stop token (partial match at the end)
            // This prevents incomplete stop tokens from appearing
            // Only check for partial matches of 2+ characters to avoid false positives on single '<' or '['
            if stop_token.len() > 2 {
                let trimmed = test_response.trim_end();
                for i in 2..stop_token.len() {
                    if trimmed.ends_with(&stop_token[..i]) {
                        println!("Debug: Stopping generation due to partial stop token: '{}' (partial: '{}')",
                                 stop_token, &stop_token[..i]);

                        // Check if the partial exists in the current response (before adding new token)
                        // If so, we need to remove it
                        if response.trim_end().ends_with(&stop_token[..i-token_str.len()]) && i > token_str.len() {
                            partial_to_remove = i - token_str.len();
                        }

                        should_stop = true;
                        break;
                    }
                }
                if should_stop {
                    break;
                }
            }
        }

        if should_stop {
            // Remove any partial stop token from the response
            if partial_to_remove > 0 {
                let new_len = response.len().saturating_sub(partial_to_remove);
                response.truncate(new_len);
                println!("Debug: Removed {} characters of partial stop token from response", partial_to_remove);
            }
            hit_stop_condition = true;
            break;
        }

        // If no stop sequence detected, add the token to the response
        response.push_str(&token_str);

        // Send token through channel for streaming (if enabled)
        if let Some(ref sender) = token_sender {
            let token_data = TokenData {
                token: token_str.clone(),
                tokens_used: token_pos,
                max_tokens: context_size as i32,
            };
            let _ = sender.send(token_data);
        }

        // Log token to conversation file
        {
            let mut logger = conversation_logger.lock().map_err(|_| "Failed to lock conversation logger")?;
            logger.log_token(&token_str);
        }
    } // End of inner token generation loop

        // Check if response contains a command to execute
        if response.contains("<COMMAND>") && response.contains("</COMMAND>") {
            if let Some(start) = response.find("<COMMAND>") {
                if let Some(end) = response.find("</COMMAND>") {
                    if end > start {
                        let command_text = &response[start + 9..end]; // 9 is length of "<COMMAND>"
                        println!("Debug: Executing command: {}", command_text);

                        // Execute the command
                        let output = execute_command(command_text);
                        println!("Debug: Command output: {}", output);

                        // Log command execution
                        {
                            let mut logger = conversation_logger.lock().map_err(|_| "Failed to lock conversation logger")?;
                            logger.log_command_execution(command_text, &output);
                        }

                        // Replace the command in the response with output
                        let before_command = &response[..start];
                        let after_command = &response[end + 10..]; // 10 is length of "</COMMAND>"

                        let command_output_text = format!(
                            "\n\n[COMMAND: {}]\n\n```\n{}\n```\n\n",
                            command_text,
                            output.trim()
                        );

                        response = format!(
                            "{}{}{}",
                            before_command.trim(),
                            command_output_text,
                            after_command
                        );

                        // Log the command output to conversation
                        {
                            let mut logger = conversation_logger.lock().map_err(|_| "Failed to lock conversation logger")?;
                            logger.log_token(&command_output_text);
                        }

                        // Tokenize the command output and feed it to the context
                        let output_tokens = model
                            .str_to_token(&command_output_text, AddBos::Never)
                            .map_err(|e| format!("Tokenization of command output failed: {}", e))?;

                        println!("Debug: Feeding {} command output tokens to context", output_tokens.len());

                        // Feed output tokens to context
                        for token in output_tokens {
                            batch.clear();
                            batch
                                .add(token, token_pos, &[0], true)
                                .map_err(|e| format!("Batch add failed for command output: {}", e))?;

                            context
                                .decode(&mut batch)
                                .map_err(|e| format!("Decode failed for command output: {}", e))?;

                            token_pos += 1;
                        }

                        command_executed = true;
                    }
                }
            }
        }

        // Break if we hit a stop condition (EOS/stop token) or reached token limit
        // Continue if we just completed a generation cycle without hitting stop
        if hit_stop_condition || total_tokens_generated >= max_total_tokens {
            println!("[DEBUG] Exiting outer loop: hit_stop_condition={}, total_tokens_generated={}, max_total_tokens={}",
                     hit_stop_condition, total_tokens_generated, max_total_tokens);
            break;
        }

        // Only continue outer loop if command was executed (for command execution workflow)
        if !command_executed {
            // No command executed and no stop condition - continue generating
            println!("[DEBUG] Continuing generation: no stop condition hit");
        }
    } // End of outer command execution loop

    // Finish the assistant message with proper formatting
    {
        let mut logger = conversation_logger.lock().map_err(|_| "Failed to lock conversation logger")?;
        logger.finish_assistant_message();
    }

    Ok((response.trim().to_string(), token_pos, max_total_tokens))
}

// WebSocket handler for real-time token streaming
async fn handle_websocket(
    upgraded: Upgraded,
    llama_state: Option<SharedLlamaState>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Convert the upgraded connection to a WebSocket stream
    let ws_stream = WebSocketStream::from_raw_socket(
        upgraded,
        tokio_tungstenite::tungstenite::protocol::Role::Server,
        None,
    ).await;

    let (mut ws_sender, mut ws_receiver) = ws_stream.split();

    let _conn_count = ACTIVE_WS_CONNECTIONS.fetch_add(1, Ordering::SeqCst) + 1;
    eprintln!("[WS_CHAT] New WebSocket connection established");

    // Wait for the first message from the client (should be the chat request)
    while let Some(msg) = ws_receiver.next().await {
        match msg {
            Ok(WsMessage::Text(text)) => {
                eprintln!("[WS_CHAT] Received message: {}", text.chars().take(100).collect::<String>());

                // Parse the chat request
                let chat_request: ChatRequest = match serde_json::from_str(&text) {
                    Ok(req) => req,
                    Err(_e) => {
                        let error_msg = serde_json::json!({
                            "type": "error",
                            "error": "Invalid JSON format"
                        });
                        let _ = ws_sender.send(WsMessage::Text(error_msg.to_string())).await;
                        break;
                    }
                };

                // Load configuration to get system prompt
                let config = load_config();
                let system_prompt = config.system_prompt.as_deref();

                // Create or load conversation logger based on conversation_id
                let conversation_logger = match &chat_request.conversation_id {
                    Some(conv_id) => {
                        eprintln!("[WS_CHAT] Loading existing conversation: {}", conv_id);
                        // Load existing conversation
                        match ConversationLogger::from_existing(conv_id) {
                            Ok(logger) => {
                                eprintln!("[WS_CHAT] Successfully loaded conversation: {}", conv_id);
                                Arc::new(Mutex::new(logger))
                            },
                            Err(e) => {
                                eprintln!("[WS_CHAT] Failed to load conversation: {}", e);
                                let error_msg = serde_json::json!({
                                    "type": "error",
                                    "error": format!("Failed to load conversation: {}", e)
                                });
                                let _ = ws_sender.send(WsMessage::Text(error_msg.to_string())).await;
                                break;
                            }
                        }
                    }
                    None => {
                        eprintln!("[WS_CHAT] Creating new conversation");
                        // Create a new conversation
                        match ConversationLogger::new(system_prompt) {
                            Ok(logger) => {
                                eprintln!("[WS_CHAT] Successfully created new conversation");
                                Arc::new(Mutex::new(logger))
                            },
                            Err(e) => {
                                eprintln!("[WS_CHAT] Failed to create conversation: {}", e);
                                let error_msg = serde_json::json!({
                                    "type": "error",
                                    "error": format!("Failed to create conversation logger: {}", e)
                                });
                                let _ = ws_sender.send(WsMessage::Text(error_msg.to_string())).await;
                                break;
                            }
                        }
                    }
                };

                // Get conversation ID to send back to client
                let conversation_id = {
                    let logger = conversation_logger.lock().unwrap();
                    logger.get_conversation_id()
                };
                eprintln!("[WS_CHAT] Conversation ID: {}", conversation_id);
                eprintln!("[WS_CHAT] User message: {}", chat_request.message);

                // Create channel for streaming tokens
                let (tx, mut rx) = mpsc::unbounded_channel::<TokenData>();

                // Spawn generation task
                let message = chat_request.message.clone();

                let state_clone = llama_state.clone();
                eprintln!("[WS_CHAT] Spawning generation task");
                tokio::spawn(async move {
                    eprintln!("[WS_CHAT] Generation task started");
                    if let Some(state) = state_clone {
                        match generate_llama_response(&message, state, conversation_logger, Some(tx)).await {
                            Ok((_content, _tokens, _max)) => {
                            }
                            Err(_e) => {
                            }
                        }
                    } else {
                    }
                });


                // Stream tokens through WebSocket
                loop {
                    tokio::select! {
                        // Receive tokens from the generation task
                        token_result = rx.recv() => {
                            match token_result {
                                Some(token_data) => {
                                    // Send token as JSON
                                    let json = serde_json::json!({
                                        "type": "token",
                                        "token": token_data.token,
                                        "tokens_used": token_data.tokens_used,
                                        "max_tokens": token_data.max_tokens
                                    });

                                    if let Err(_e) = ws_sender.send(WsMessage::Text(json.to_string())).await {
                                        break;
                                    }
                                }
                                None => {
                                    // Channel closed, generation complete
                                    eprintln!("[WS_CHAT] Generation complete, sending done message");
                                    let done_msg = serde_json::json!({
                                        "type": "done",
                                        "conversation_id": conversation_id
                                    });
                                    let _ = ws_sender.send(WsMessage::Text(done_msg.to_string())).await;
                                    eprintln!("[WS_CHAT] Done message sent");
                                    break;
                                }
                            }
                        }
                        // Handle client disconnection or close messages
                        ws_msg = ws_receiver.next() => {
                            match ws_msg {
                                Some(Ok(WsMessage::Close(_))) | None => {
                                    break;
                                }
                                Some(Ok(WsMessage::Ping(data))) => {
                                    let _ = ws_sender.send(WsMessage::Pong(data)).await;
                                }
                                Some(Err(_e)) => {
                                    break;
                                }
                                _ => {}
                            }
                        }
                    }
                }
                eprintln!("[WS_CHAT] Message processing complete, waiting for next message");
                // Don't break - keep WebSocket open for subsequent messages
            }
            Ok(WsMessage::Close(_)) => {
                eprintln!("[WS_CHAT] Received Close message");
                break;
            }
            Ok(WsMessage::Ping(data)) => {
                let _ = ws_sender.send(WsMessage::Pong(data)).await;
            }
            Ok(_) => {
                // Ignore other message types
            }
            Err(_e) => {
                break;
            }
        }
    }

    let _conn_count = ACTIVE_WS_CONNECTIONS.fetch_sub(1, Ordering::SeqCst) - 1;
    eprintln!("[WS_CHAT] WebSocket connection closed");
    Ok(())
}

// WebSocket handler for watching conversation file changes
async fn handle_conversation_watch(
    upgraded: Upgraded,
    conversation_id: String,
    llama_state: Option<SharedLlamaState>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let ws_stream = WebSocketStream::from_raw_socket(
        upgraded,
        tokio_tungstenite::tungstenite::protocol::Role::Server,
        None,
    ).await;

    let (mut ws_sender, mut ws_receiver) = ws_stream.split();

    let _ = ACTIVE_WS_CONNECTIONS.fetch_add(1, Ordering::SeqCst);

    // Construct file path - handle .txt extension if already present
    let file_path = if conversation_id.ends_with(".txt") {
        format!("assets/conversations/{}", conversation_id)
    } else {
        format!("assets/conversations/{}.txt", conversation_id)
    };

    // Read initial content
    let mut last_content = fs::read_to_string(&file_path).unwrap_or_default();

    // Calculate token counts for initial content
    let (tokens_used, max_tokens) = if let Some(ref state) = llama_state {
        if let Ok(state_lock) = state.lock() {
            let model = &state_lock.model;
            let context_size = state_lock.context.n_ctx() as i32;

            // Apply chat template to get the prompt
            let template_type = state_lock.chat_template_type.as_deref();
            match apply_model_chat_template(&last_content, template_type) {
                Ok(prompt) => {
                    match model.str_to_token(&prompt, llama_cpp_2::model::AddBos::Always) {
                        Ok(tokens) => (Some(tokens.len() as i32), Some(context_size)),
                        Err(_) => (None, Some(context_size))
                    }
                },
                Err(_) => (None, Some(context_size))
            }
        } else {
            (None, None)
        }
    } else {
        (None, None)
    };

    // Send initial content with token info
    let initial_msg = serde_json::json!({
        "type": "update",
        "content": last_content,
        "tokens_used": tokens_used,
        "max_tokens": max_tokens
    });

    let _ = ws_sender.send(WsMessage::Text(initial_msg.to_string())).await;

    // Poll for file changes every 500ms
    let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(500));

    loop {
        tokio::select! {
            _ = interval.tick() => {
                // Read file and check if changed
                if let Ok(current_content) = fs::read_to_string(&file_path) {
                    if current_content != last_content {
                        last_content = current_content.clone();

                        // Calculate token counts for updated content
                        let (tokens_used, max_tokens) = if let Some(ref state) = llama_state {
                            if let Ok(state_lock) = state.lock() {
                                let model = &state_lock.model;
                                let context_size = state_lock.context.n_ctx() as i32;

                                // Apply chat template to get the prompt
                                let template_type = state_lock.chat_template_type.as_deref();
                                match apply_model_chat_template(&current_content, template_type) {
                                    Ok(prompt) => {
                                        match model.str_to_token(&prompt, llama_cpp_2::model::AddBos::Always) {
                                            Ok(tokens) => (Some(tokens.len() as i32), Some(context_size)),
                                            Err(_) => (None, Some(context_size))
                                        }
                                    },
                                    Err(_) => (None, Some(context_size))
                                }
                            } else {
                                (None, None)
                            }
                        } else {
                            (None, None)
                        };

                        let update_msg = serde_json::json!({
                            "type": "update",
                            "content": current_content,
                            "tokens_used": tokens_used,
                            "max_tokens": max_tokens
                        });

                        if ws_sender.send(WsMessage::Text(update_msg.to_string())).await.is_err() {
                            break;
                        }
                    }
                }
            }
            ws_msg = ws_receiver.next() => {
                match ws_msg {
                    Some(Ok(WsMessage::Close(_))) | None => {
                        break;
                    }
                    Some(Ok(WsMessage::Ping(data))) => {
                        let _ = ws_sender.send(WsMessage::Pong(data)).await;
                    }
                    Some(Err(_)) => {
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    let _ = ACTIVE_WS_CONNECTIONS.fetch_sub(1, Ordering::SeqCst);
    Ok(())
}


async fn handle_request(
    req: Request<Body>,
    llama_state: SharedLlamaState,
) -> std::result::Result<Response<Body>, Infallible> {
    handle_request_impl(req, Some(llama_state)).await
}

#[cfg(feature = "mock")]
async fn handle_request(
    req: Request<Body>,
) -> std::result::Result<Response<Body>, Infallible> {
    handle_request_impl(req, None).await
}

async fn handle_request_impl(
    req: Request<Body>,
    
    llama_state: Option<SharedLlamaState>,
    #[cfg(feature = "mock")]
    _llama_state: Option<()>,
) -> std::result::Result<Response<Body>, Infallible> {
    let response = match (req.method(), req.uri().path()) {
        (&Method::GET, "/health") => {
            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .header("access-control-allow-origin", "*")
                .body(Body::from(r#"{"status":"ok","service":"llama-chat-web"}"#))
                .unwrap()
        }
        
        (&Method::POST, "/api/chat") => {
            // Parse request body
            let body_bytes = match hyper::body::to_bytes(req.into_body()).await {
                Ok(bytes) => bytes,
                Err(_) => {
                    return Ok(Response::builder()
                        .status(StatusCode::BAD_REQUEST)
                        .header("content-type", "application/json")
                        .header("access-control-allow-origin", "*")
                        .body(Body::from(r#"{"error":"Failed to read request body"}"#))
                        .unwrap());
                }
            };

            // Debug: log the received JSON
            if let Ok(body_str) = std::str::from_utf8(&body_bytes) {
                println!("Received request body: {}", body_str);
            }
            
            let chat_request: ChatRequest = match serde_json::from_slice(&body_bytes) {
                Ok(req) => req,
                Err(e) => {
                    println!("JSON parsing error: {}", e);
                    return Ok(Response::builder()
                        .status(StatusCode::BAD_REQUEST)
                        .header("content-type", "application/json")
                        .header("access-control-allow-origin", "*")
                        .body(Body::from(r#"{"error":"Invalid JSON format"}"#))
                        .unwrap());
                }
            };

            
            {
                // Check for test mode environment variable
                if std::env::var("TEST_MODE").unwrap_or_default() == "true" {
                    // Fast test response
                    let test_response = format!(
                        "Hello! This is a test response to your message: '{}'", 
                        chat_request.message
                    );
                    
                    let response = ChatResponse {
                        message: ChatMessage {
                            id: format!("{}", uuid::Uuid::new_v4()),
                            role: "assistant".to_string(),
                            content: test_response,
                            timestamp: std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs(),
                        },
                        conversation_id: chat_request.conversation_id.unwrap_or_else(|| format!("{}", uuid::Uuid::new_v4())),
                        tokens_used: None,
                        max_tokens: None,
                    };
                    
                    return Ok(Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .header("access-control-allow-origin", "*")
                        .body(Body::from(serde_json::to_string(&response).unwrap()))
                        .unwrap());
                }
                
                // Create or load conversation logger
                let conversation_logger = if let Some(conversation_id) = &chat_request.conversation_id {
                    // Load existing conversation
                    match ConversationLogger::from_existing(conversation_id) {
                        Ok(logger) => Arc::new(Mutex::new(logger)),
                        Err(e) => {
                            return Ok(Response::builder()
                                .status(StatusCode::INTERNAL_SERVER_ERROR)
                                .header("content-type", "application/json")
                                .header("access-control-allow-origin", "*")
                                .body(Body::from(format!(r#"{{"error":"Failed to load conversation: {}"}}"#, e)))
                                .unwrap());
                        }
                    }
                } else {
                    // Create new conversation
                    let config = load_config();
                    let system_prompt = config.system_prompt.as_deref();

                    match ConversationLogger::new(system_prompt) {
                        Ok(logger) => Arc::new(Mutex::new(logger)),
                        Err(e) => {
                            return Ok(Response::builder()
                                .status(StatusCode::INTERNAL_SERVER_ERROR)
                                .header("content-type", "application/json")
                                .header("access-control-allow-origin", "*")
                                .body(Body::from(format!(r#"{{"error":"Failed to create conversation logger: {}"}}"#, e)))
                                .unwrap());
                        }
                    }
                };
                
                // Generate actual LLaMA response
                let (response_content, tokens_used, max_tokens) = match llama_state {
                    Some(state) => {
                        match generate_llama_response(&chat_request.message, state, conversation_logger.clone(), None).await {
                            Ok((content, tokens, max_tok)) => (content, Some(tokens), Some(max_tok)),
                            Err(err) => (format!("Error generating response: {}", err), None, None),
                        }
                    }
                    None => ("LLaMA state not available".to_string(), None, None),
                };

                // Extract conversation ID from the logger's file path
                let conversation_id = {
                    let logger = conversation_logger.lock().unwrap();
                    let file_path = &logger.file_path;
                    // Extract filename from path: "assets/conversations/chat_xxx.txt" -> "chat_xxx.txt"
                    file_path.split('/').last().unwrap_or("unknown").to_string()
                };

                let chat_response = ChatResponse {
                    message: ChatMessage {
                        id: uuid::Uuid::new_v4().to_string(),
                        role: "assistant".to_string(),
                        content: response_content,
                        timestamp: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs(),
                    },
                    conversation_id,
                    tokens_used,
                    max_tokens,
                };

                let response_json = match serde_json::to_string(&chat_response) {
                    Ok(json) => json,
                    Err(_) => r#"{"error":"Failed to serialize response"}"#.to_string(),
                };

                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .header("access-control-allow-origin", "*")
                    .header("access-control-allow-methods", "GET, POST, OPTIONS")
                    .header("access-control-allow-headers", "content-type")
                    .body(Body::from(response_json))
                    .unwrap()
            }

            #[cfg(feature = "mock")]
            {
                // Fallback mock response when using mock feature
                let mock_response = r#"{"message":{"id":"test","role":"assistant","content":"LLaMA integration not available (mock feature enabled)","timestamp":1234567890},"conversation_id":"test-conversation"}"#;
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .header("access-control-allow-origin", "*")
                    .header("access-control-allow-methods", "GET, POST, OPTIONS")
                    .header("access-control-allow-headers", "content-type")
                    .body(Body::from(mock_response))
                    .unwrap()
            }
        }

        (&Method::POST, "/api/chat/stream") => {
            // Parse request body
            let body_bytes = match hyper::body::to_bytes(req.into_body()).await {
                Ok(bytes) => bytes,
                Err(_) => {
                    return Ok(Response::builder()
                        .status(StatusCode::BAD_REQUEST)
                        .header("content-type", "application/json")
                        .header("access-control-allow-origin", "*")
                        .body(Body::from(r#"{"error":"Failed to read request body"}"#))
                        .unwrap());
                }
            };

            let chat_request: ChatRequest = match serde_json::from_slice(&body_bytes) {
                Ok(req) => req,
                Err(e) => {
                    println!("JSON parsing error: {}", e);
                    return Ok(Response::builder()
                        .status(StatusCode::BAD_REQUEST)
                        .header("content-type", "application/json")
                        .header("access-control-allow-origin", "*")
                        .body(Body::from(r#"{"error":"Invalid JSON format"}"#))
                        .unwrap());
                }
            };

            
            {
                // Load configuration to get system prompt
                let config = load_config();
                let system_prompt = config.system_prompt.as_deref();

                // Create a new conversation logger for this chat session
                let conversation_logger = match ConversationLogger::new(system_prompt) {
                    Ok(logger) => Arc::new(Mutex::new(logger)),
                    Err(e) => {
                        return Ok(Response::builder()
                            .status(StatusCode::INTERNAL_SERVER_ERROR)
                            .header("content-type", "application/json")
                            .header("access-control-allow-origin", "*")
                            .body(Body::from(format!(r#"{{"error":"Failed to create conversation logger: {}"}}"#, e)))
                            .unwrap());
                    }
                };

                // Create channel for streaming tokens
                let (tx, mut rx) = mpsc::unbounded_channel::<TokenData>();

                // Spawn generation task
                let message = chat_request.message.clone();
                let state_clone = llama_state.clone();
                tokio::spawn(async move {
                    if let Some(state) = state_clone {
                        match generate_llama_response(&message, state, conversation_logger, Some(tx)).await {
                            Ok((_content, tokens, max)) => {
                                println!("[DEBUG] Generation completed successfully: {} tokens used, {} max", tokens, max);
                            }
                            Err(e) => {
                                println!("[ERROR] Generation failed: {}", e);
                            }
                        }
                    } else {
                        println!("[ERROR] No LLaMA state available for generation");
                    }
                });

                // Use Body::channel for direct control over chunk sending
                let (mut sender, body) = Body::channel();

                // Spawn task to send tokens through the channel
                tokio::spawn(async move {
                    loop {
                        match rx.recv().await {
                            Some(token_data) => {
                                // Send TokenData as JSON
                                let json = serde_json::to_string(&token_data).unwrap_or_else(|_| r#"{"token":"","tokens_used":0,"max_tokens":0}"#.to_string());
                                let event = format!("data: {}\n\n", json);

                                // Send chunk immediately - this ensures no buffering
                                if sender.send_data(Bytes::from(event)).await.is_err() {
                                    // Client disconnected
                                    break;
                                }
                            }
                            None => {
                                // Channel closed, generation complete
                                break;
                            }
                        }
                    }
                    // Send done event
                    let _ = sender.send_data(Bytes::from("data: [DONE]\n\n")).await;
                });

                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/event-stream")
                    .header("cache-control", "no-cache")
                    .header("access-control-allow-origin", "*")
                    .header("connection", "keep-alive")
                    .header("x-accel-buffering", "no")  // Disable nginx buffering
                    .body(body)
                    .unwrap()
            }

            #[cfg(feature = "mock")]
            {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .header("access-control-allow-origin", "*")
                    .body(Body::from(r#"{"error":"Streaming not available (mock feature enabled)"}"#))
                    .unwrap()
            }
        }

        (&Method::GET, "/ws/chat/stream") => {

            // Check if the request wants to upgrade to WebSocket
            let upgrade_header = req.headers().get("upgrade")
                .and_then(|v| v.to_str().ok())
                .map(|v| v.to_lowercase());

            if upgrade_header.as_deref() != Some("websocket") {
                return Ok(Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .header("content-type", "application/json")
                    .header("access-control-allow-origin", "*")
                    .body(Body::from(r#"{"error":"WebSocket upgrade required"}"#))
                    .unwrap());
            }

            // Extract the WebSocket key before moving req
            let key = req.headers()
                .get("sec-websocket-key")
                .and_then(|k| k.to_str().ok())
                .unwrap_or("")
                .to_string();

            // Calculate accept key using the WebSocket protocol
            let accept_key = {
                use sha1::{Digest, Sha1};
                use base64::{Engine as _, engine::general_purpose};

                let mut hasher = Sha1::new();
                hasher.update(key.as_bytes());
                hasher.update(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
                let hash = hasher.finalize();
                general_purpose::STANDARD.encode(hash)
            };

            // Clone state for the WebSocket handler
            let llama_state_ws = llama_state.clone();

            // Spawn WebSocket handler on the upgraded connection
            tokio::spawn(async move {
                match hyper::upgrade::on(req).await {
                    Ok(upgraded) => {
                        if let Err(e) = handle_websocket(upgraded, llama_state_ws).await {
                            println!("[WEBSOCKET ERROR] {}", e);
                        }
                    }
                    Err(e) => {
                        println!("[WEBSOCKET UPGRADE ERROR] {}", e);
                    }
                }
            });

            // Return 101 Switching Protocols response
            Response::builder()
                .status(StatusCode::SWITCHING_PROTOCOLS)
                .header("upgrade", "websocket")
                .header("connection", "upgrade")
                .header("sec-websocket-accept", accept_key)
                .body(Body::empty())
                .unwrap()
        }

        (&Method::GET, path) if path.starts_with("/ws/conversation/watch/") => {
            println!("[CONV-WATCH] Received request to {}", path);

            // Extract conversation ID from path
            let conversation_id = path.trim_start_matches("/ws/conversation/watch/").to_string();

            // Check if the request wants to upgrade to WebSocket
            let upgrade_header = req.headers().get("upgrade")
                .and_then(|v| v.to_str().ok())
                .map(|v| v.to_lowercase());

            if upgrade_header.as_deref() != Some("websocket") {
                return Ok(Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .header("content-type", "application/json")
                    .header("access-control-allow-origin", "*")
                    .body(Body::from(r#"{"error":"WebSocket upgrade required"}"#))
                    .unwrap());
            }

            // Extract the WebSocket key
            let key = req.headers()
                .get("sec-websocket-key")
                .and_then(|k| k.to_str().ok())
                .unwrap_or("")
                .to_string();

            // Calculate accept key
            let accept_key = {
                use sha1::{Digest, Sha1};
                use base64::{Engine as _, engine::general_purpose};

                let mut hasher = Sha1::new();
                hasher.update(key.as_bytes());
                hasher.update(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
                let hash = hasher.finalize();
                general_purpose::STANDARD.encode(hash)
            };

            // Spawn WebSocket handler
            let state_for_watch = llama_state.clone();
            tokio::spawn(async move {
                match hyper::upgrade::on(req).await {
                    Ok(upgraded) => {
                        if let Err(e) = handle_conversation_watch(upgraded, conversation_id, state_for_watch).await {
                            println!("[CONV-WATCH ERROR] {}", e);
                        }
                    }
                    Err(e) => {
                        println!("[CONV-WATCH UPGRADE ERROR] {}", e);
                    }
                }
            });

            // Return 101 Switching Protocols response
            Response::builder()
                .status(StatusCode::SWITCHING_PROTOCOLS)
                .header("upgrade", "websocket")
                .header("connection", "upgrade")
                .header("sec-websocket-accept", accept_key)
                .body(Body::empty())
                .unwrap()
        }

        (&Method::GET, "/api/config") => {
            // Load current configuration from file or use defaults
            let config = load_config();
            
            match serde_json::to_string(&config) {
                Ok(config_json) => {
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .header("access-control-allow-origin", "*")
                        .body(Body::from(config_json))
                        .unwrap()
                }
                Err(_) => {
                    Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .header("content-type", "application/json")
                        .header("access-control-allow-origin", "*")
                        .body(Body::from(r#"{"error":"Failed to serialize configuration"}"#))
                        .unwrap()
                }
            }
        }
        
        (&Method::POST, "/api/config") => {
            // Parse request body for configuration update
            let body_bytes = match hyper::body::to_bytes(req.into_body()).await {
                Ok(bytes) => bytes,
                Err(_) => {
                    return Ok(Response::builder()
                        .status(StatusCode::BAD_REQUEST)
                        .header("content-type", "application/json")
                        .header("access-control-allow-origin", "*")
                        .body(Body::from(r#"{"error":"Failed to read request body"}"#))
                        .unwrap());
                }
            };

            // Parse incoming config
            let incoming_config: SamplerConfig = match serde_json::from_slice(&body_bytes) {
                Ok(config) => config,
                Err(_) => {
                    return Ok(Response::builder()
                        .status(StatusCode::BAD_REQUEST)
                        .header("content-type", "application/json")
                        .header("access-control-allow-origin", "*")
                        .body(Body::from(r#"{"error":"Invalid JSON format"}"#))
                        .unwrap());
                }
            };

            // Load existing config to preserve model_history
            let mut existing_config = load_config();

            // Update fields from incoming config, but preserve model_history
            existing_config.sampler_type = incoming_config.sampler_type;
            existing_config.temperature = incoming_config.temperature;
            existing_config.top_p = incoming_config.top_p;
            existing_config.top_k = incoming_config.top_k;
            existing_config.mirostat_tau = incoming_config.mirostat_tau;
            existing_config.mirostat_eta = incoming_config.mirostat_eta;
            existing_config.model_path = incoming_config.model_path;
            existing_config.system_prompt = incoming_config.system_prompt;
            existing_config.context_size = incoming_config.context_size;
            existing_config.stop_tokens = incoming_config.stop_tokens;
            // Note: model_history is NOT updated from incoming config

            // Save merged configuration to file
            let config_path = "assets/config.json";
            if let Err(_) = fs::create_dir_all("assets") {
                return Ok(Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .header("content-type", "application/json")
                    .header("access-control-allow-origin", "*")
                    .body(Body::from(r#"{"error":"Failed to create config directory"}"#))
                    .unwrap());
            }

            match fs::write(config_path, serde_json::to_string_pretty(&existing_config).unwrap_or_default()) {
                Ok(_) => {
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .header("access-control-allow-origin", "*")
                        .body(Body::from(r#"{"success":true}"#))
                        .unwrap()
                }
                Err(_) => {
                    Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .header("content-type", "application/json")
                        .header("access-control-allow-origin", "*")
                        .body(Body::from(r#"{"error":"Failed to save configuration"}"#))
                        .unwrap()
                }
            }
        }
        
        (&Method::GET, path) if path.starts_with("/api/conversation/") => {
            // Extract filename from path: /api/conversation/{filename}
            let filename = &path[18..]; // Remove "/api/conversation/"
            let conversations_dir = "assets/conversations";
            let file_path = format!("{}/{}", conversations_dir, filename);
            
            match fs::read_to_string(&file_path) {
                Ok(content) => {
                    let messages = parse_conversation_to_messages(&content);
                    let response = ConversationContentResponse {
                        content: content.clone(),
                        messages,
                    };
                    
                    let response_json = match serde_json::to_string(&response) {
                        Ok(json) => json,
                        Err(_) => r#"{"content":"","messages":[]}"#.to_string(),
                    };
                    
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .header("access-control-allow-origin", "*")
                        .body(Body::from(response_json))
                        .unwrap()
                }
                Err(_) => {
                    Response::builder()
                        .status(StatusCode::NOT_FOUND)
                        .header("content-type", "application/json")
                        .header("access-control-allow-origin", "*")
                        .body(Body::from(r#"{"error":"Conversation not found"}"#))
                        .unwrap()
                }
            }
        }
        
        (&Method::GET, "/api/model/info") => {
            println!("[DEBUG] /api/model/info endpoint hit");

            // Extract model path from query parameters
            let query = req.uri().query().unwrap_or("");
            println!("[DEBUG] Query string: {}", query);

            let mut model_path = "";

            for param in query.split('&') {
                if let Some((key, value)) = param.split_once('=') {
                    if key == "path" {
                        // URL decode the path
                        model_path = value;
                        println!("[DEBUG] Found path parameter (encoded): {}", model_path);
                        break;
                    }
                }
            }

            if model_path.is_empty() {
                println!("[DEBUG] ERROR: No path parameter provided");
                return Ok(Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .header("content-type", "application/json")
                    .header("access-control-allow-origin", "*")
                    .body(Body::from(r#"{"error":"Model path is required"}"#))
                    .unwrap());
            }

            // URL decode the path properly
            let decoded_path = urlencoding::decode(model_path).unwrap_or(std::borrow::Cow::Borrowed(model_path));
            println!("[DEBUG] Decoded path: {}", decoded_path);

            // Check if file exists
            let path_obj = std::path::Path::new(&*decoded_path);
            let exists = path_obj.exists();
            println!("[DEBUG] File exists: {}", exists);
            println!("[DEBUG] Path is file: {}", path_obj.is_file());
            println!("[DEBUG] Path is dir: {}", path_obj.is_dir());

            if !exists {
                println!("[DEBUG] ERROR: File does not exist at path: {}", decoded_path);
                return Ok(Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .header("content-type", "application/json")
                    .header("access-control-allow-origin", "*")
                    .body(Body::from(r#"{"error":"Model file not found"}"#))
                    .unwrap());
            }

            // Check if path is a directory
            if path_obj.is_dir() {
                println!("[DEBUG] Path is a directory, scanning for .gguf files...");

                // Find all .gguf files in the directory
                let mut gguf_files = Vec::new();
                if let Ok(entries) = fs::read_dir(path_obj) {
                    for entry in entries.flatten() {
                        let entry_path = entry.path();
                        if entry_path.is_file() {
                            if let Some(ext) = entry_path.extension() {
                                if ext.eq_ignore_ascii_case("gguf") {
                                    if let Some(filename) = entry_path.file_name().and_then(|n| n.to_str()) {
                                        gguf_files.push(filename.to_string());
                                    }
                                }
                            }
                        }
                    }
                }

                let response_json = if gguf_files.is_empty() {
                    serde_json::json!({
                        "error": "This is a directory. No .gguf files found in this directory.",
                        "is_directory": true,
                        "suggestions": []
                    })
                } else {
                    serde_json::json!({
                        "error": format!("This is a directory. Found {} .gguf file(s). Please select one:", gguf_files.len()),
                        "is_directory": true,
                        "suggestions": gguf_files
                    })
                };

                println!("[DEBUG] Returning directory error with {} suggestions", gguf_files.len());
                return Ok(Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .header("content-type", "application/json")
                    .header("access-control-allow-origin", "*")
                    .body(Body::from(response_json.to_string()))
                    .unwrap());
            }

            // Check if file has .gguf extension
            if let Some(ext) = path_obj.extension() {
                if !ext.eq_ignore_ascii_case("gguf") {
                    println!("[DEBUG] ERROR: File is not a .gguf file");
                    return Ok(Response::builder()
                        .status(StatusCode::BAD_REQUEST)
                        .header("content-type", "application/json")
                        .header("access-control-allow-origin", "*")
                        .body(Body::from(r#"{"error":"File must have .gguf extension"}"#))
                        .unwrap());
                }
            } else {
                println!("[DEBUG] ERROR: File has no extension");
                return Ok(Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .header("content-type", "application/json")
                    .header("access-control-allow-origin", "*")
                    .body(Body::from(r#"{"error":"File must have .gguf extension"}"#))
                    .unwrap());
            }
            
            // Extract basic model information
            let file_metadata = match fs::metadata(&*decoded_path) {
                Ok(metadata) => metadata,
                Err(_) => {
                    return Ok(Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .header("content-type", "application/json")
                        .header("access-control-allow-origin", "*")
                        .body(Body::from(r#"{"error":"Failed to read file metadata"}"#))
                        .unwrap());
                }
            };
            
            let file_size_bytes = file_metadata.len();
            let file_size = if file_size_bytes >= 1_073_741_824 {
                format!("{:.1} GB", file_size_bytes as f64 / 1_073_741_824.0)
            } else if file_size_bytes >= 1_048_576 {
                format!("{:.1} MB", file_size_bytes as f64 / 1_048_576.0)
            } else {
                format!("{} bytes", file_size_bytes)
            };
            
            let filename = std::path::Path::new(&*decoded_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown");
            
            // Try to extract model information from filename patterns
            let mut architecture = "Unknown";
            let mut parameters = "Unknown";
            let mut quantization = "Unknown";
            
            // Common GGUF naming patterns
            if filename.contains("llama") || filename.contains("Llama") {
                architecture = "LLaMA";
            } else if filename.contains("mistral") || filename.contains("Mistral") {
                architecture = "Mistral";
            } else if filename.contains("qwen") || filename.contains("Qwen") {
                architecture = "Qwen";
            } else if filename.contains("phi") || filename.contains("Phi") {
                architecture = "Phi";
            }
            
            // Extract parameter count
            if filename.contains("7b") || filename.contains("7B") {
                parameters = "7B";
            } else if filename.contains("13b") || filename.contains("13B") {
                parameters = "13B";
            } else if filename.contains("70b") || filename.contains("70B") {
                parameters = "70B";
            } else if filename.contains("1.5b") || filename.contains("1.5B") {
                parameters = "1.5B";
            } else if filename.contains("3b") || filename.contains("3B") {
                parameters = "3B";
            }
            
            // Extract quantization
            if filename.contains("q4_0") || filename.contains("Q4_0") {
                quantization = "Q4_0";
            } else if filename.contains("q4_1") || filename.contains("Q4_1") {
                quantization = "Q4_1";
            } else if filename.contains("q5_0") || filename.contains("Q5_0") {
                quantization = "Q5_0";
            } else if filename.contains("q5_1") || filename.contains("Q5_1") {
                quantization = "Q5_1";
            } else if filename.contains("q8_0") || filename.contains("Q8_0") {
                quantization = "Q8_0";
            } else if filename.contains("f16") || filename.contains("F16") {
                quantization = "F16";
            } else if filename.contains("f32") || filename.contains("F32") {
                quantization = "F32";
            }
            
            // Estimate total layers based on model size
            let model_size_gb = file_size_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
            let estimated_total_layers = if model_size_gb < 8.0 {
                36  // Small models (7B and below)
            } else if model_size_gb < 15.0 {
                45  // Medium models (13B)
            } else if model_size_gb < 25.0 {
                60  // Large models (30B)
            } else {
                80  // Very large models (70B+)
            };

            // Build base model info
            let mut model_info = serde_json::json!({
                "name": filename,
                "architecture": architecture,
                "parameters": parameters,
                "quantization": quantization,
                "file_size": file_size,
                "context_length": "Variable",
                "path": decoded_path.to_string(),
                "estimated_layers": estimated_total_layers
            });

            // Try to parse GGUF metadata
            if let Ok(file) = fs::File::open(&*decoded_path) {
                let mut reader = BufReader::new(file);

                if let Ok(header) = GgufHeader::parse(&mut reader) {
                    if let Ok(metadata) = GgufReader::read_metadata(&mut reader, header.n_kv) {
                        // Debug: Print all available metadata keys and values
                        println!("=== GGUF Metadata Found ===");
                        for (key, value) in metadata.iter() {
                            let val_str = match value {
                                Value::String(s) => format!("\"{}\"", s),
                                Value::Uint8(n) => n.to_string(),
                                Value::Uint16(n) => n.to_string(),
                                Value::Uint32(n) => n.to_string(),
                                Value::Uint64(n) => n.to_string(),
                                Value::Int8(n) => n.to_string(),
                                Value::Int16(n) => n.to_string(),
                                Value::Int32(n) => n.to_string(),
                                Value::Int64(n) => n.to_string(),
                                Value::Float32(f) => f.to_string(),
                                Value::Float64(f) => f.to_string(),
                                Value::Bool(b) => b.to_string(),
                                Value::Array(_, items) => format!("[Array with {} items]", items.len()),
                            };
                            println!("  {} = {}", key, val_str);
                        }
                        println!("================================");

                        // Helper to get metadata value as string
                        let get_meta_string = |key: &str| -> Option<String> {
                            metadata.get(key).and_then(|v| match v {
                                Value::String(s) => Some(s.clone()),
                                Value::Uint8(n) => Some(n.to_string()),
                                Value::Uint16(n) => Some(n.to_string()),
                                Value::Uint32(n) => Some(n.to_string()),
                                Value::Uint64(n) => Some(n.to_string()),
                                Value::Int8(n) => Some(n.to_string()),
                                Value::Int16(n) => Some(n.to_string()),
                                Value::Int32(n) => Some(n.to_string()),
                                Value::Int64(n) => Some(n.to_string()),
                                Value::Float32(f) => Some(f.to_string()),
                                Value::Float64(f) => Some(f.to_string()),
                                Value::Bool(b) => Some(b.to_string()),
                                _ => None,
                            })
                        };

                        // Create a metadata object with all values
                        let mut all_metadata = serde_json::Map::new();
                        for (key, value) in metadata.iter() {
                            let val_json = match value {
                                Value::String(s) => serde_json::json!(s),
                                Value::Uint8(n) => serde_json::json!(n),
                                Value::Uint16(n) => serde_json::json!(n),
                                Value::Uint32(n) => serde_json::json!(n),
                                Value::Uint64(n) => serde_json::json!(n),
                                Value::Int8(n) => serde_json::json!(n),
                                Value::Int16(n) => serde_json::json!(n),
                                Value::Int32(n) => serde_json::json!(n),
                                Value::Int64(n) => serde_json::json!(n),
                                Value::Float32(f) => serde_json::json!(f),
                                Value::Float64(f) => serde_json::json!(f),
                                Value::Bool(b) => serde_json::json!(b),
                                Value::Array(_, _) => serde_json::json!("[Array]"),
                            };
                            all_metadata.insert(key.clone(), val_json);
                        }
                        model_info["gguf_metadata"] = serde_json::json!(all_metadata);

                        // Get architecture
                        let arch = get_meta_string("general.architecture")
                            .unwrap_or_else(|| "llama".to_string());

                        // Update architecture
                        model_info["architecture"] = serde_json::json!(arch.clone());

                        // Detect tool calling format based on architecture and model name
                        let model_name = get_meta_string("general.name").unwrap_or_default().to_lowercase();
                        let tool_format = if arch.contains("mistral") || model_name.contains("mistral") || model_name.contains("devstral") {
                            "mistral"
                        } else if arch.contains("llama") && (model_name.contains("llama-3") || model_name.contains("llama3")) {
                            "llama3"
                        } else if arch.contains("qwen") || model_name.contains("qwen") {
                            "qwen"
                        } else if arch.contains("llama") {
                            // Older llama models don't support tools
                            "unknown"
                        } else {
                            "unknown"
                        };
                        model_info["tool_format"] = serde_json::json!(tool_format);

                        // Core model information
                        if let Some(val) = get_meta_string("general.name") {
                            model_info["general_name"] = serde_json::json!(val);
                        }
                        if let Some(val) = get_meta_string("general.author") {
                            model_info["author"] = serde_json::json!(val);
                        }
                        if let Some(val) = get_meta_string("general.version") {
                            model_info["version"] = serde_json::json!(val);
                        }
                        if let Some(val) = get_meta_string("general.organization") {
                            model_info["organization"] = serde_json::json!(val);
                        }
                        if let Some(val) = get_meta_string("general.description") {
                            model_info["description"] = serde_json::json!(val);
                        }
                        if let Some(val) = get_meta_string("general.license") {
                            model_info["license"] = serde_json::json!(val);
                        }
                        if let Some(val) = get_meta_string("general.url") {
                            model_info["url"] = serde_json::json!(val);
                        }
                        if let Some(val) = get_meta_string("general.repo_url") {
                            model_info["repo_url"] = serde_json::json!(val);
                        }
                        if let Some(val) = get_meta_string("general.file_type") {
                            model_info["file_type"] = serde_json::json!(val);
                        }
                        if let Some(val) = get_meta_string("general.quantization_version") {
                            model_info["quantization_version"] = serde_json::json!(val);
                        }

                        // Context length - try multiple keys
                        let context_keys = vec![
                            format!("{}.context_length", arch),
                            "llama.context_length".to_string(),
                            "context_length".to_string(),
                        ];
                        for key in &context_keys {
                            if let Some(val) = get_meta_string(key) {
                                model_info["context_length"] = serde_json::json!(val);
                                break;
                            }
                        }

                        // Architecture-specific fields
                        if let Some(val) = get_meta_string(&format!("{}.embedding_length", arch)) {
                            model_info["embedding_length"] = serde_json::json!(val);
                        }
                        if let Some(val) = get_meta_string(&format!("{}.block_count", arch)) {
                            model_info["block_count"] = serde_json::json!(val.clone());
                            // Use actual block count for layers
                            if let Ok(block_count) = val.parse::<u32>() {
                                model_info["estimated_layers"] = serde_json::json!(block_count);
                            }
                        }
                        if let Some(val) = get_meta_string(&format!("{}.feed_forward_length", arch)) {
                            model_info["feed_forward_length"] = serde_json::json!(val);
                        }
                        if let Some(val) = get_meta_string(&format!("{}.attention.head_count", arch)) {
                            model_info["attention_head_count"] = serde_json::json!(val);
                        }
                        if let Some(val) = get_meta_string(&format!("{}.attention.head_count_kv", arch)) {
                            model_info["attention_head_count_kv"] = serde_json::json!(val);
                        }
                        if let Some(val) = get_meta_string(&format!("{}.attention.layer_norm_rms_epsilon", arch)) {
                            model_info["layer_norm_epsilon"] = serde_json::json!(val);
                        }
                        if let Some(val) = get_meta_string(&format!("{}.rope.dimension_count", arch)) {
                            model_info["rope_dimension_count"] = serde_json::json!(val);
                        }
                        if let Some(val) = get_meta_string(&format!("{}.rope.freq_base", arch)) {
                            model_info["rope_freq_base"] = serde_json::json!(val);
                        }

                        // Tokenizer information
                        if let Some(val) = get_meta_string("tokenizer.ggml.model") {
                            model_info["tokenizer_model"] = serde_json::json!(val);
                        }
                        if let Some(val) = get_meta_string("tokenizer.ggml.bos_token_id") {
                            model_info["bos_token_id"] = serde_json::json!(val);
                        }
                        if let Some(val) = get_meta_string("tokenizer.ggml.eos_token_id") {
                            model_info["eos_token_id"] = serde_json::json!(val);
                        }
                        if let Some(val) = get_meta_string("tokenizer.ggml.padding_token_id") {
                            model_info["padding_token_id"] = serde_json::json!(val);
                        }
                        if let Some(val) = get_meta_string("tokenizer.chat_template") {
                            model_info["chat_template"] = serde_json::json!(val);

                            // Extract default system prompt from chat template
                            // Look for: {%- set default_system_message = '...' %}
                            if let Some(start_idx) = val.find("set default_system_message = '") {
                                let after_start = &val[start_idx + "set default_system_message = '".len()..];
                                if let Some(end_idx) = after_start.find("' %}") {
                                    let default_prompt = &after_start[..end_idx];
                                    model_info["default_system_prompt"] = serde_json::json!(default_prompt);
                                }
                            }
                        }
                    }
                }
            }
            
            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .header("access-control-allow-origin", "*")
                .body(Body::from(model_info.to_string()))
                .unwrap()
        }
        
        (&Method::GET, "/api/browse") => {
            // Parse query parameters for path
            let query = req.uri().query().unwrap_or("");
            let mut browse_path = "/app/models"; // Default path
            
            // Simple query parameter parsing
            for param in query.split('&') {
                if let Some((key, value)) = param.split_once('=') {
                    if key == "path" {
                        // Simple path assignment (assume already decoded by browser)
                        browse_path = value;
                    }
                }
            }
            
            // Security: ensure path is within allowed directories
            let allowed_paths = ["/app/models", "/app"];
            let is_allowed = allowed_paths.iter().any(|&allowed| {
                browse_path.starts_with(allowed)
            });
            
            if !is_allowed {
                return Ok(Response::builder()
                    .status(StatusCode::FORBIDDEN)
                    .header("content-type", "application/json")
                    .header("access-control-allow-origin", "*")
                    .body(Body::from(r#"{"error":"Path not allowed"}"#))
                    .unwrap());
            }
            
            let mut files = Vec::new();
            let current_path = browse_path.to_string();
            let parent_path = if browse_path != "/app/models" && browse_path != "/app" {
                std::path::Path::new(browse_path)
                    .parent()
                    .and_then(|p| p.to_str())
                    .map(|s| s.to_string())
            } else {
                None
            };
            
            match fs::read_dir(browse_path) {
                Ok(entries) => {
                    for entry in entries {
                        if let Ok(entry) = entry {
                            let path = entry.path();
                            if let (Some(name), Some(path_str)) = (
                                path.file_name().and_then(|n| n.to_str()),
                                path.to_str()
                            ) {
                                let is_directory = path.is_dir();
                                let size = if !is_directory {
                                    entry.metadata().ok().map(|m| m.len())
                                } else {
                                    None
                                };
                                
                                files.push(FileItem {
                                    name: name.to_string(),
                                    path: path_str.to_string(),
                                    is_directory,
                                    size,
                                });
                            }
                        }
                    }
                    
                    // Sort: directories first, then files, both alphabetically
                    files.sort_by(|a, b| {
                        match (a.is_directory, b.is_directory) {
                            (true, false) => std::cmp::Ordering::Less,
                            (false, true) => std::cmp::Ordering::Greater,
                            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                        }
                    });
                }
                Err(e) => {
                    eprintln!("Failed to read directory {}: {}", browse_path, e);
                    return Ok(Response::builder()
                        .status(StatusCode::NOT_FOUND)
                        .header("content-type", "application/json")
                        .header("access-control-allow-origin", "*")
                        .body(Body::from(r#"{"error":"Directory not found"}"#))
                        .unwrap());
                }
            }
            
            let response = BrowseFilesResponse {
                files,
                current_path,
                parent_path,
            };
            
            let response_json = match serde_json::to_string(&response) {
                Ok(json) => json,
                Err(_) => r#"{"files":[],"current_path":"/app/models"}"#.to_string(),
            };
            
            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .header("access-control-allow-origin", "*")
                .body(Body::from(response_json))
                .unwrap()
        }
        
        (&Method::GET, "/api/model/status") => {
            
            {
                if let Some(state) = llama_state {
                    let status = get_model_status(&state);
                    let response_json = match serde_json::to_string(&status) {
                        Ok(json) => json,
                        Err(_) => r#"{"loaded":false,"model_path":null,"last_used":null,"memory_usage_mb":null}"#.to_string(),
                    };

                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .header("access-control-allow-origin", "*")
                        .body(Body::from(response_json))
                        .unwrap()
                } else {
                    Response::builder()
                        .status(StatusCode::SERVICE_UNAVAILABLE)
                        .header("content-type", "application/json")
                        .header("access-control-allow-origin", "*")
                        .body(Body::from(r#"{"loaded":false,"model_path":null,"last_used":null,"memory_usage_mb":null}"#))
                        .unwrap()
                }
            }

            #[cfg(feature = "mock")]
            {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .header("access-control-allow-origin", "*")
                    .body(Body::from(r#"{"loaded":false,"model_path":null,"last_used":null,"memory_usage_mb":null}"#))
                    .unwrap()
            }
        }

        (&Method::GET, "/api/model/history") => {
            // Load config and return model history
            let config = load_config();
            let response_json = match serde_json::to_string(&config.model_history) {
                Ok(json) => json,
                Err(_) => "[]".to_string(),
            };

            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .header("access-control-allow-origin", "*")
                .body(Body::from(response_json))
                .unwrap()
        }

        (&Method::POST, "/api/model/load") => {
            println!("[DEBUG] /api/model/load endpoint hit");

            
            {
                if let Some(state) = llama_state {
                    // Parse request body
                    let body_bytes = match hyper::body::to_bytes(req.into_body()).await {
                        Ok(bytes) => bytes,
                        Err(e) => {
                            println!("[DEBUG] Failed to read request body: {}", e);
                            return Ok(Response::builder()
                                .status(StatusCode::BAD_REQUEST)
                                .header("content-type", "application/json")
                                .header("access-control-allow-origin", "*")
                                .body(Body::from(r#"{"success":false,"message":"Failed to read request body"}"#))
                                .unwrap());
                        }
                    };

                    println!("[DEBUG] Request body: {}", String::from_utf8_lossy(&body_bytes));

                    let load_request: ModelLoadRequest = match serde_json::from_slice(&body_bytes) {
                        Ok(req) => req,
                        Err(e) => {
                            println!("[DEBUG] JSON parsing error in model/load: {}", e);
                            println!("[DEBUG] Raw body was: {}", String::from_utf8_lossy(&body_bytes));
                            return Ok(Response::builder()
                                .status(StatusCode::BAD_REQUEST)
                                .header("content-type", "application/json")
                                .header("access-control-allow-origin", "*")
                                .body(Body::from(r#"{"success":false,"message":"Invalid JSON format"}"#))
                                .unwrap());
                        }
                    };

                    // Attempt to load the model
                    match load_model(state.clone(), &load_request.model_path).await {
                        Ok(_) => {
                            // Add to model history on successful load
                            add_to_model_history(&load_request.model_path);

                            let status = get_model_status(&state);
                            let response = ModelResponse {
                                success: true,
                                message: format!("Model loaded successfully from {}", load_request.model_path),
                                status: Some(status),
                            };

                            let response_json = match serde_json::to_string(&response) {
                                Ok(json) => json,
                                Err(_) => r#"{"success":true,"message":"Model loaded successfully","status":null}"#.to_string(),
                            };

                            Response::builder()
                                .status(StatusCode::OK)
                                .header("content-type", "application/json")
                                .header("access-control-allow-origin", "*")
                                .body(Body::from(response_json))
                                .unwrap()
                        }
                        Err(e) => {
                            let response = ModelResponse {
                                success: false,
                                message: format!("Failed to load model: {}", e),
                                status: None,
                            };
                            
                            let response_json = match serde_json::to_string(&response) {
                                Ok(json) => json,
                                Err(_) => format!(r#"{{"success":false,"message":"Failed to load model: {}","status":null}}"#, e),
                            };
                            
                            Response::builder()
                                .status(StatusCode::INTERNAL_SERVER_ERROR)
                                .header("content-type", "application/json")
                                .header("access-control-allow-origin", "*")
                                .body(Body::from(response_json))
                                .unwrap()
                        }
                    }
                } else {
                    Response::builder()
                        .status(StatusCode::SERVICE_UNAVAILABLE)
                        .header("content-type", "application/json")
                        .header("access-control-allow-origin", "*")
                        .body(Body::from(r#"{"success":false,"message":"LLaMA state not available"}"#))
                        .unwrap()
                }
            }
            
            #[cfg(feature = "mock")]
            {
                Response::builder()
                    .status(StatusCode::SERVICE_UNAVAILABLE)
                    .header("content-type", "application/json")
                    .header("access-control-allow-origin", "*")
                    .body(Body::from(r#"{"success":false,"message":"Model loading not available (mock feature enabled)"}"#))
                    .unwrap()
            }
        }
        
        (&Method::POST, "/api/model/unload") => {
            
            {
                if let Some(state) = llama_state {
                    match unload_model(state.clone()).await {
                        Ok(_) => {
                            let status = get_model_status(&state);
                            let response = ModelResponse {
                                success: true,
                                message: "Model unloaded successfully".to_string(),
                                status: Some(status),
                            };
                            
                            let response_json = match serde_json::to_string(&response) {
                                Ok(json) => json,
                                Err(_) => r#"{"success":true,"message":"Model unloaded successfully","status":null}"#.to_string(),
                            };
                            
                            Response::builder()
                                .status(StatusCode::OK)
                                .header("content-type", "application/json")
                                .header("access-control-allow-origin", "*")
                                .body(Body::from(response_json))
                                .unwrap()
                        }
                        Err(e) => {
                            let response = ModelResponse {
                                success: false,
                                message: format!("Failed to unload model: {}", e),
                                status: None,
                            };
                            
                            let response_json = match serde_json::to_string(&response) {
                                Ok(json) => json,
                                Err(_) => format!(r#"{{"success":false,"message":"Failed to unload model: {}","status":null}}"#, e),
                            };
                            
                            Response::builder()
                                .status(StatusCode::INTERNAL_SERVER_ERROR)
                                .header("content-type", "application/json")
                                .header("access-control-allow-origin", "*")
                                .body(Body::from(response_json))
                                .unwrap()
                        }
                    }
                } else {
                    Response::builder()
                        .status(StatusCode::SERVICE_UNAVAILABLE)
                        .header("content-type", "application/json")
                        .header("access-control-allow-origin", "*")
                        .body(Body::from(r#"{"success":false,"message":"LLaMA state not available"}"#))
                        .unwrap()
                }
            }
            
            #[cfg(feature = "mock")]
            {
                Response::builder()
                    .status(StatusCode::SERVICE_UNAVAILABLE)
                    .header("content-type", "application/json")
                    .header("access-control-allow-origin", "*")
                    .body(Body::from(r#"{"success":false,"message":"Model unloading not available (mock feature enabled)"}"#))
                    .unwrap()
            }
        }
        
        (&Method::POST, "/api/upload") => {
            // Extract headers before consuming the request body
            let content_disposition = req.headers().get("content-disposition")
                .and_then(|h| h.to_str().ok())
                .unwrap_or("")
                .to_string();
                
            let query = req.uri().query().unwrap_or("").to_string();

            // Handle file upload
            let body_bytes = match hyper::body::to_bytes(req.into_body()).await {
                Ok(bytes) => bytes,
                Err(_) => {
                    return Ok(Response::builder()
                        .status(StatusCode::BAD_REQUEST)
                        .header("content-type", "application/json")
                        .header("access-control-allow-origin", "*")
                        .body(Body::from(r#"{"success":false,"message":"Failed to read request body"}"#))
                        .unwrap());
                }
            };
            
            let filename = if content_disposition.contains("filename=") {
                content_disposition
                    .split("filename=")
                    .nth(1)
                    .and_then(|s| s.split(';').next())
                    .map(|s| s.trim_matches('"'))
                    .unwrap_or("uploaded_model.gguf")
            } else {
                // Try to get filename from query parameter
                let mut filename = "uploaded_model.gguf";
                for param in query.split('&') {
                    if let Some((key, value)) = param.split_once('=') {
                        if key == "filename" {
                            filename = value;
                            break;
                        }
                    }
                }
                filename
            };

            // Ensure the filename ends with .gguf
            let filename = if filename.ends_with(".gguf") {
                filename.to_string()
            } else {
                format!("{}.gguf", filename)
            };

            // Save file to models directory
            let file_path = format!("/app/models/{}", filename);
            match fs::write(&file_path, &body_bytes) {
                Ok(_) => {
                    let response = serde_json::json!({
                        "success": true,
                        "message": "File uploaded successfully",
                        "file_path": file_path
                    });
                    
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .header("access-control-allow-origin", "*")
                        .body(Body::from(response.to_string()))
                        .unwrap()
                }
                Err(e) => {
                    let response = serde_json::json!({
                        "success": false,
                        "message": format!("Failed to save file: {}", e)
                    });
                    
                    Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .header("content-type", "application/json")
                        .header("access-control-allow-origin", "*")
                        .body(Body::from(response.to_string()))
                        .unwrap()
                }
            }
        }
        
        (&Method::GET, "/api/conversations") => {
            // Fetch conversation files from assets/conversations directory
            let conversations_dir = "assets/conversations";
            let mut conversations = Vec::new();

            match fs::read_dir(conversations_dir) {
                Ok(entries) => {
                    for entry in entries {
                        if let Ok(entry) = entry {
                            let path = entry.path();
                            if path.is_file() && path.extension().map_or(false, |ext| ext == "txt") {
                                if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
                                    // Extract timestamp from filename (chat_YYYY-MM-DD-HH-mm-ss-SSS.txt)
                                    if filename.starts_with("chat_") && filename.ends_with(".txt") {
                                        let timestamp_part = &filename[5..filename.len()-4]; // Remove "chat_" and ".txt"

                                        conversations.push(ConversationFile {
                                            name: filename.to_string(),
                                            display_name: format!("Chat {}", timestamp_part),
                                            timestamp: timestamp_part.to_string(),
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Failed to read conversations directory: {}", e);
                }
            }

            // Sort conversations by timestamp (newest first)
            conversations.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

            let response = ConversationsResponse { conversations };
            let response_json = match serde_json::to_string(&response) {
                Ok(json) => json,
                Err(_) => r#"{"conversations":[]}"#.to_string(),
            };

            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .header("access-control-allow-origin", "*")
                .body(Body::from(response_json))
                .unwrap()
        }

        (&Method::DELETE, path) if path.starts_with("/api/conversations/") => {
            // Extract filename from path
            let filename = &path["/api/conversations/".len()..];

            // Validate filename to prevent path traversal
            if filename.contains("..") || filename.contains("/") || filename.contains("\\") {
                return Ok(Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .header("content-type", "application/json")
                    .header("access-control-allow-origin", "*")
                    .body(Body::from(r#"{"error":"Invalid filename"}"#))
                    .unwrap());
            }

            // Only allow deleting .txt files that start with "chat_"
            if !filename.starts_with("chat_") || !filename.ends_with(".txt") {
                return Ok(Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .header("content-type", "application/json")
                    .header("access-control-allow-origin", "*")
                    .body(Body::from(r#"{"error":"Invalid conversation file"}"#))
                    .unwrap());
            }

            let file_path = format!("assets/conversations/{}", filename);

            match fs::remove_file(&file_path) {
                Ok(_) => {
                    println!("Deleted conversation file: {}", filename);
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .header("access-control-allow-origin", "*")
                        .body(Body::from(r#"{"success":true}"#))
                        .unwrap()
                }
                Err(e) => {
                    eprintln!("Failed to delete conversation file: {}", e);
                    Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .header("content-type", "application/json")
                        .header("access-control-allow-origin", "*")
                        .body(Body::from(r#"{"error":"Failed to delete conversation"}"#))
                        .unwrap()
                }
            }
        }

        (&Method::OPTIONS, _) => {
            Response::builder()
                .status(StatusCode::OK)
                .header("access-control-allow-origin", "*")
                .header("access-control-allow-methods", "GET, POST, DELETE, OPTIONS")
                .header("access-control-allow-headers", "content-type")
                .body(Body::empty())
                .unwrap()
        }
        
        (&Method::GET, "/") => {
            // Serve the main index.html from the built frontend
            match std::fs::read_to_string("./dist/index.html") {
                Ok(content) => Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/html")
                    .body(Body::from(content))
                    .unwrap(),
                Err(_) => {
                    // Fallback HTML if dist files aren't found
                    let html = r#"<!DOCTYPE html>
<html>
<head><title>LLaMA Chat Web</title></head>
<body>
<h1>🦙 LLaMA Chat Web Server</h1>
<p>Web server is running successfully!</p>
<p>Frontend files not found. API endpoints:</p>
<ul>
<li>GET /health - Health check</li>
<li>POST /api/chat - Chat endpoint</li>
<li>GET /api/config - Configuration</li>
</ul>
</body>
</html>"#;
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "text/html")
                        .body(Body::from(html))
                        .unwrap()
                }
            }
        }
        
        (&Method::GET, path) if path.starts_with("/assets/") || path.ends_with(".svg") || path.ends_with(".ico") || path.ends_with(".png") => {
            // Serve static assets (JS, CSS, etc.)
            let file_path = format!("./dist{}", path);
            match std::fs::read(&file_path) {
                Ok(content) => {
                    let content_type = if path.ends_with(".js") {
                        "application/javascript"
                    } else if path.ends_with(".css") {
                        "text/css"
                    } else if path.ends_with(".png") {
                        "image/png"
                    } else if path.ends_with(".jpg") || path.ends_with(".jpeg") {
                        "image/jpeg"
                    } else if path.ends_with(".svg") {
                        "image/svg+xml"
                    } else {
                        "application/octet-stream"
                    };
                    
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", content_type)
                        .header("cache-control", "public, max-age=31536000") // 1 year cache
                        .body(Body::from(content))
                        .unwrap()
                }
                Err(_) => {
                    Response::builder()
                        .status(StatusCode::NOT_FOUND)
                        .body(Body::from("Asset not found"))
                        .unwrap()
                }
            }
        }

        (&Method::POST, "/api/tools/execute") => {
            // Parse request body
            let body_bytes = match hyper::body::to_bytes(req.into_body()).await {
                Ok(bytes) => bytes,
                Err(_) => {
                    return Ok(Response::builder()
                        .status(StatusCode::BAD_REQUEST)
                        .header("content-type", "application/json")
                        .header("access-control-allow-origin", "*")
                        .body(Body::from(r#"{"error":"Failed to read request body"}"#))
                        .unwrap());
                }
            };

            #[derive(serde::Deserialize)]
            struct ToolExecuteRequest {
                tool_name: String,
                arguments: serde_json::Value,
            }

            let request: ToolExecuteRequest = match serde_json::from_slice(&body_bytes) {
                Ok(req) => req,
                Err(e) => {
                    println!("JSON parsing error: {}", e);
                    return Ok(Response::builder()
                        .status(StatusCode::BAD_REQUEST)
                        .header("content-type", "application/json")
                        .header("access-control-allow-origin", "*")
                        .body(Body::from(r#"{"error":"Invalid JSON format"}"#))
                        .unwrap());
                }
            };

            // Execute tool based on name
            let result = match request.tool_name.as_str() {
                "bash" | "shell" | "command" => {
                    // Extract command from arguments
                    let command = request.arguments.get("command")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");

                    if command.is_empty() {
                        return Ok(Response::builder()
                            .status(StatusCode::BAD_REQUEST)
                            .header("content-type", "application/json")
                            .header("access-control-allow-origin", "*")
                            .body(Body::from(r#"{"error":"Command is required"}"#))
                            .unwrap());
                    }

                    // Execute command (with timeout for safety)
                    let output = if cfg!(target_os = "windows") {
                        std::process::Command::new("cmd")
                            .args(["/C", command])
                            .output()
                    } else {
                        std::process::Command::new("sh")
                            .arg("-c")
                            .arg(command)
                            .output()
                    };

                    match output {
                        Ok(output) => {
                            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                            let combined = if !stderr.is_empty() {
                                format!("{}\nSTDERR:\n{}", stdout, stderr)
                            } else {
                                stdout
                            };

                            serde_json::json!({
                                "success": true,
                                "result": combined,
                                "exit_code": output.status.code()
                            })
                        }
                        Err(e) => {
                            serde_json::json!({
                                "success": false,
                                "error": format!("Failed to execute command: {}", e)
                            })
                        }
                    }
                }
                _ => {
                    serde_json::json!({
                        "success": false,
                        "error": format!("Unknown tool: {}", request.tool_name)
                    })
                }
            };

            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .header("access-control-allow-origin", "*")
                .body(Body::from(result.to_string()))
                .unwrap()
        }

        _ => {
            Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::from("Not Found"))
                .unwrap()
        }
    };
    
    Ok(response)
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    // Create shared LLaMA state
    
    let llama_state: SharedLlamaState = Arc::new(Mutex::new(None));
    
    // Note: ConversationLogger will be created per chat request, not globally
    
    // Create HTTP service
    let make_svc = make_service_fn({
        
        let llama_state = llama_state.clone();
        
        move |_conn| {
            
            let llama_state = llama_state.clone();
            
            async move {
                Ok::<_, Infallible>(service_fn(move |req| {
                    
                    {
                        handle_request(req, llama_state.clone())
                    }
                    #[cfg(feature = "mock")]
                    {
                        handle_request(req)
                    }
                }))
            }
        }
    });
    
    // Start server
    let addr = SocketAddr::from(([0, 0, 0, 0], 8000));
    let server = Server::bind(&addr).serve(make_svc);
    
    println!("🦙 LLaMA Chat Web Server starting on http://{}", addr);
    println!("Available endpoints:");
    println!("  GET  /health               - Health check");
    println!("  POST /api/chat             - Chat with LLaMA");
    println!("  GET  /api/config           - Get sampler configuration");
    println!("  POST /api/config           - Update sampler configuration");
    println!("  GET  /api/model/status     - Get current model status");
    println!("  POST /api/model/load       - Load a specific model");
    println!("  POST /api/model/unload     - Unload current model");
    println!("  POST /api/upload           - Upload model file");
    println!("  GET  /api/conversations    - List conversation files");
    println!("  POST /api/tools/execute    - Execute tool calls");
    println!("  GET  /api/browse           - Browse model files");
    println!("  GET  /                     - Web interface");
    
    server.await.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    
    Ok(())
}