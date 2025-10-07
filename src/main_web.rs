// Simple web server version of LLaMA Chat (without Tauri)
use std::net::SocketAddr;
use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use std::num::NonZeroU32;
use std::fs;
use std::io;
use serde::{Deserialize, Serialize};
use serde_json;

// HTTP server using hyper
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Method, Request, Response, Server, StatusCode};

// LLaMA integration
#[cfg(feature = "docker")]
use llama_cpp_2::{
    context::params::LlamaContextParams,
    llama_backend::LlamaBackend,
    llama_batch::LlamaBatch,
    model::{params::LlamaModelParams, AddBos, LlamaModel, Special},
    sampling::LlamaSampler,
    // send_logs_to_tracing, LogOptions,
};

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
}

// Common stop tokens for different model providers
fn get_common_stop_tokens() -> Vec<String> {
    vec![
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
        // Default system prompt from test.rs
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
            model_path: Some("/app/models/lmstudio-community/granite-4.0-h-tiny-GGUF/granite-4.0-h-tiny-Q4_K_M.gguf".to_string()),
            system_prompt: Some(default_system_prompt.trim().to_string()),
            context_size: Some(32768),
            stop_tokens: Some(get_common_stop_tokens()),
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
#[cfg(feature = "docker")]
struct LlamaState {
    backend: LlamaBackend,
    model: Option<LlamaModel>,
    current_model_path: Option<String>,
    last_used: std::time::SystemTime,
}

#[cfg(feature = "docker")]
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

// Helper function to get model status
#[cfg(feature = "docker")]
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

// Helper function to load a model
#[cfg(feature = "docker")]
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
    
    // Load new model with GPU acceleration
    let model_params = LlamaModelParams::default()
        .with_n_gpu_layers(32);

    println!("Loading model from: {}", model_path);
    println!("GPU layers enabled: 32 layers will be offloaded to GPU");

    let model = LlamaModel::load_from_file(&state.backend, model_path, &model_params)
        .map_err(|e| format!("Failed to load model: {}", e))?;

    println!("Model loaded successfully!");
    
    state.model = Some(model);
    state.current_model_path = Some(model_path.to_string());
    state.last_used = std::time::SystemTime::now();
    
    Ok(())
}

// Helper function to unload the current model
#[cfg(feature = "docker")]
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
        
        // Add system prompt at the beginning - use configured prompt or fallback
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
        
        let prompt_to_use = system_prompt.unwrap_or(default_system_prompt.trim());
        logger.log_message("SYSTEM", prompt_to_use);
        
        Ok(logger)
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
        
        // Write immediately to file
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

    fn get_full_conversation(&self) -> String {
        // Return the complete conversation content from memory
        self.content.clone()
    }

    fn load_conversation_from_file(&self) -> io::Result<String> {
        // Read the conversation directly from file (source of truth)
        fs::read_to_string(&self.file_path)
    }

    fn save(&self) -> io::Result<()> {
        // Final save (content should already be written, but ensure it's there)
        fs::write(&self.file_path, &self.content)?;
        println!("Conversation saved to: {}", self.file_path);
        Ok(())
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

fn convert_conversation_to_chat_format(conversation: &str) -> String {
    let mut chat_format = String::new();
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
                let role_tag = match current_role {
                    "SYSTEM" => "system",
                    "USER" => "user",
                    "ASSISTANT" => "assistant",
                    _ => "user",
                };
                chat_format.push_str(&format!(
                    "<|start_of_role|>{}<|end_of_role|>{}<|end_of_text|>",
                    role_tag,
                    current_content.trim()
                ));
            }

            // Start new role
            current_role = line.trim_end_matches(":");
            current_content.clear();
        } else if !line.starts_with("[COMMAND:") {
            // Skip command execution logs in this conversion, add content
            if !line.trim().is_empty() {
                current_content.push_str(line);
                current_content.push('\n');
            }
        }
    }

    // Add the final role content
    if !current_role.is_empty() && !current_content.trim().is_empty() {
        let role_tag = match current_role {
            "SYSTEM" => "system",
            "USER" => "user",
            "ASSISTANT" => "assistant",
            _ => "user",
        };
        chat_format.push_str(&format!(
            "<|start_of_role|>{}<|end_of_role|>{}<|end_of_text|>",
            role_tag,
            current_content.trim()
        ));
    }

    // Add assistant start for response generation
    chat_format.push_str("<|start_of_role|>assistant<|end_of_role|>");

    chat_format
}

// Constants for LLaMA configuration
#[cfg(feature = "docker")]
const CONTEXT_SIZE: u32 = 32768;
#[cfg(feature = "docker")]
const MODEL_PATH: &str = "/app/models/lmstudio-community/granite-4.0-h-tiny-GGUF/granite-4.0-h-tiny-Q4_K_M.gguf";

#[cfg(feature = "docker")]
async fn generate_llama_response(user_message: &str, llama_state: SharedLlamaState, conversation_logger: SharedConversationLogger) -> Result<String, String> {
    // Log user message to conversation file
    {
        let mut logger = conversation_logger.lock().map_err(|_| "Failed to lock conversation logger")?;
        logger.log_message("USER", user_message);
    }
    
    // Load configuration to get model path and context size
    let config = load_config();
    let model_path = config.model_path.as_deref().unwrap_or(MODEL_PATH);
    let context_size = config.context_size.unwrap_or(CONTEXT_SIZE);
    let stop_tokens = config.stop_tokens.unwrap_or_else(get_common_stop_tokens);
    
    // Ensure model is loaded
    load_model(llama_state.clone(), model_path).await?;
    
    // Now use the shared state for generation
    let state_guard = llama_state.lock().map_err(|_| "Failed to lock LLaMA state")?;
    let state = state_guard.as_ref().ok_or("LLaMA state not initialized")?;
    let model = state.model.as_ref().ok_or("No model loaded")?;
    
    // Create sampler
    let mut sampler = LlamaSampler::greedy();
    
    // Read conversation history from file and create chat prompt
    let conversation_content = {
        let logger = conversation_logger.lock().map_err(|_| "Failed to lock conversation logger")?;
        logger.load_conversation_from_file().unwrap_or_else(|_| logger.get_full_conversation())
    };
    
    // Convert conversation to chat format for LLaMA
    let prompt = convert_conversation_to_chat_format(&conversation_content);
    
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
    
    for _ in 0..512 { // Limit response length
        // Sample next token
        let next_token = sampler.sample(&context, -1);

        // Check for end-of-sequence token
        if next_token == model.token_eos() {
            println!("Debug: Stopping generation due to EOS token");
            break;
        }

        // Convert to string
        let token_str = model
            .token_to_str(next_token, Special::Tokenize)
            .map_err(|e| format!("Token conversion failed: {}", e))?;

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

        for stop_token in &stop_tokens {
            // Check if the test response contains the complete stop token
            if test_response.contains(stop_token) {
                println!("Debug: Stopping generation due to stop token detected: '{}'", stop_token);
                should_stop = true;
                break;
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
            break;
        }

        // If no stop sequence detected, add the token to the response
        response.push_str(&token_str);
        
        // Log token to conversation file
        {
            let mut logger = conversation_logger.lock().map_err(|_| "Failed to lock conversation logger")?;
            logger.log_token(&token_str);
        }
        
        // Prepare next iteration
        batch.clear();
        batch
            .add(next_token, token_pos, &[0], true)
            .map_err(|e| format!("Batch add failed: {}", e))?;
        
        context
            .decode(&mut batch)
            .map_err(|e| format!("Decode failed: {}", e))?;
        
        token_pos += 1;
    }
    
    // Finish the assistant message with proper formatting
    {
        let mut logger = conversation_logger.lock().map_err(|_| "Failed to lock conversation logger")?;
        logger.finish_assistant_message();
    }
    
    Ok(response.trim().to_string())
}

#[cfg(feature = "docker")]
async fn handle_request(
    req: Request<Body>,
    llama_state: SharedLlamaState,
) -> std::result::Result<Response<Body>, Infallible> {
    handle_request_impl(req, Some(llama_state)).await
}

#[cfg(not(feature = "docker"))]
async fn handle_request(
    req: Request<Body>,
) -> std::result::Result<Response<Body>, Infallible> {
    handle_request_impl(req, None).await
}

async fn handle_request_impl(
    req: Request<Body>,
    #[cfg(feature = "docker")]
    llama_state: Option<SharedLlamaState>,
    #[cfg(not(feature = "docker"))]
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

            #[cfg(feature = "docker")]
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
                    };
                    
                    return Ok(Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .header("access-control-allow-origin", "*")
                        .body(Body::from(serde_json::to_string(&response).unwrap()))
                        .unwrap());
                }
                
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
                
                // Generate actual LLaMA response
                let response_content = match llama_state {
                    Some(state) => {
                        match generate_llama_response(&chat_request.message, state, conversation_logger).await {
                            Ok(content) => content,
                            Err(err) => format!("Error generating response: {}", err),
                        }
                    }
                    None => "LLaMA state not available".to_string(),
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
                    conversation_id: chat_request.conversation_id
                        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
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

            #[cfg(not(feature = "docker"))]
            {
                // Fallback mock response when not using docker feature
                let mock_response = r#"{"message":{"id":"test","role":"assistant","content":"LLaMA integration not available (docker feature not enabled)","timestamp":1234567890},"conversation_id":"test-conversation"}"#;
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

            // Validate JSON structure
            let config_json: serde_json::Value = match serde_json::from_slice(&body_bytes) {
                Ok(json) => json,
                Err(_) => {
                    return Ok(Response::builder()
                        .status(StatusCode::BAD_REQUEST)
                        .header("content-type", "application/json")
                        .header("access-control-allow-origin", "*")
                        .body(Body::from(r#"{"error":"Invalid JSON format"}"#))
                        .unwrap());
                }
            };

            // Save configuration to file
            let config_path = "assets/config.json";
            if let Err(_) = fs::create_dir_all("assets") {
                return Ok(Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .header("content-type", "application/json")
                    .header("access-control-allow-origin", "*")
                    .body(Body::from(r#"{"error":"Failed to create config directory"}"#))
                    .unwrap());
            }

            match fs::write(config_path, serde_json::to_string_pretty(&config_json).unwrap_or_default()) {
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
            
            let model_info = serde_json::json!({
                "name": filename,
                "architecture": architecture,
                "parameters": parameters,
                "quantization": quantization,
                "file_size": file_size,
                "context_length": "Variable", // GGUF models can have different context lengths
                "path": decoded_path.to_string()
            });
            
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
            #[cfg(feature = "docker")]
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
            
            #[cfg(not(feature = "docker"))]
            {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .header("access-control-allow-origin", "*")
                    .body(Body::from(r#"{"loaded":false,"model_path":null,"last_used":null,"memory_usage_mb":null}"#))
                    .unwrap()
            }
        }
        
        (&Method::POST, "/api/model/load") => {
            println!("[DEBUG] /api/model/load endpoint hit");

            #[cfg(feature = "docker")]
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
            
            #[cfg(not(feature = "docker"))]
            {
                Response::builder()
                    .status(StatusCode::SERVICE_UNAVAILABLE)
                    .header("content-type", "application/json")
                    .header("access-control-allow-origin", "*")
                    .body(Body::from(r#"{"success":false,"message":"Model loading not available (docker feature not enabled)"}"#))
                    .unwrap()
            }
        }
        
        (&Method::POST, "/api/model/unload") => {
            #[cfg(feature = "docker")]
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
            
            #[cfg(not(feature = "docker"))]
            {
                Response::builder()
                    .status(StatusCode::SERVICE_UNAVAILABLE)
                    .header("content-type", "application/json")
                    .header("access-control-allow-origin", "*")
                    .body(Body::from(r#"{"success":false,"message":"Model unloading not available (docker feature not enabled)"}"#))
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
        
        (&Method::OPTIONS, _) => {
            Response::builder()
                .status(StatusCode::OK)
                .header("access-control-allow-origin", "*")
                .header("access-control-allow-methods", "GET, POST, OPTIONS")
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
    #[cfg(feature = "docker")]
    let llama_state: SharedLlamaState = Arc::new(Mutex::new(None));
    
    // Note: ConversationLogger will be created per chat request, not globally
    
    // Create HTTP service
    let make_svc = make_service_fn({
        #[cfg(feature = "docker")]
        let llama_state = llama_state.clone();
        
        move |_conn| {
            #[cfg(feature = "docker")]
            let llama_state = llama_state.clone();
            
            async move {
                Ok::<_, Infallible>(service_fn(move |req| {
                    #[cfg(feature = "docker")]
                    {
                        handle_request(req, llama_state.clone())
                    }
                    #[cfg(not(feature = "docker"))]
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
    println!("  GET  /api/browse           - Browse model files");
    println!("  GET  /                     - Web interface");
    
    server.await.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    
    Ok(())
}