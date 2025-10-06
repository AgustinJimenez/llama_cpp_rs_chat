use std::collections::HashMap;
use std::sync::{Arc, Mutex};
#[cfg(not(feature = "docker"))]
use tauri::State;
use serde::{Deserialize, Serialize};

// Re-export the chat logic - conditional compilation for Docker vs local
#[cfg(feature = "docker")]
pub mod chat;
#[cfg(feature = "docker")]
use chat::{ChatEngine, ChatConfig, SamplerType};

#[cfg(not(feature = "docker"))]
pub mod chat_mock;
#[cfg(not(feature = "docker"))]
use chat_mock::{ChatEngine, ChatConfig, SamplerType};

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
            model_path: Some("/app/models/lmstudio-community/granite-4.0-h-tiny-GGUF/granite-4.0-h-tiny-Q4_K_M.gguf".to_string()),
            system_prompt: Some(default_system_prompt.trim().to_string()),
        }
    }
}

// Tauri commands (exposed to frontend)
#[cfg(not(feature = "docker"))]
pub async fn send_message(
    request: ChatRequest,
    state: State<'_, AppState>,
) -> Result<ChatResponse, String> {
    let conversation_id = request.conversation_id.unwrap_or_else(|| {
        uuid::Uuid::new_v4().to_string()
    });
    
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
        let conversation = conversations.entry(conversation_id.clone()).or_insert_with(Vec::new);
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
    
    // Generate AI response using a new chat engine instance
    let ai_response_content = match ChatEngine::new(chat_config) {
        Ok(engine) => {
            engine.generate_response(&request.message).await
                .unwrap_or_else(|e| format!("Error generating response: {}", e))
        }
        Err(e) => format!("Error creating chat engine: {}", e)
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

#[cfg(not(feature = "docker"))]
pub async fn get_conversations(
    state: State<'_, AppState>,
) -> Result<HashMap<String, Vec<Message>>, String> {
    let conversations = state.conversations.lock().unwrap();
    Ok(conversations.clone())
}

#[cfg(not(feature = "docker"))]
pub async fn get_conversation(
    conversation_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<Message>, String> {
    let conversations = state.conversations.lock().unwrap();
    Ok(conversations.get(&conversation_id).cloned().unwrap_or_default())
}

#[cfg(not(feature = "docker"))]
pub async fn get_sampler_config() -> Result<SamplerConfig, String> {
    // Return the default config for now
    // In a real app, this would read from a config file or database
    Ok(SamplerConfig::default())
}

#[cfg(not(feature = "docker"))]
pub async fn update_sampler_config(
    config: SamplerConfig,
) -> Result<(), String> {
    // Store the updated configuration
    // Note: This will require reinitializing the chat engine with new config
    println!("Updated sampler config: {:?}", config);
    // In a real implementation, we'd save this to a file or database
    // and potentially restart the chat engine with the new configuration
    Ok(())
}