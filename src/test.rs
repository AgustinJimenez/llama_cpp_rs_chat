use std::io::{self, Write};
use std::fs;
use std::path::Path;
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};

const SYSTEM_PROMPT: &str = "You are a helpful AI assistant with command-line access.

You can execute system commands by using the !CMD! syntax:
- Format: !CMD!command!CMD!
- Example: !CMD!ls -la!CMD! to list files
- Example: !CMD!pwd!CMD! to show current directory

Always be helpful and provide clear explanations for your actions.";
const DEFAULT_MODEL_PATH: &str = "E:\\.lmstudio\\models\\lmstudio-community\\Qwen3-Coder-30B-A3B-Instruct-1M-UD-Q8_K_XL-GGUF\\Qwen3-Coder-30B-A3B-Instruct-1M-UD-Q8_K_XL.gguf";
const DEFAULT_CONTEXT_SIZE: u32 = 32000;

fn main() {
    let model_path = get_model_path_from_user();
    let context_size = get_context_size_from_user();
    
    println!("Selected model path: {}", model_path);
    println!("Selected context size: {}", context_size);
    
    let conversation_file = create_conversation_file(&model_path, context_size);
    add_system_prompt(&conversation_file);
    println!("Started conversation: {}", conversation_file);
    println!("Type '/exit' to quit.\n");
    
    loop {
        match get_input("You: ") {
            Some(message) => {
                if message == "/exit" {
                    println!("Goodbye!");
                    break;
                }
                
                add_message(&conversation_file, "user", &message);
                chat(&message);
            }
            None => {
                println!("\nGoodbye!");
                break;
            }
        }
    }
}

fn get_input(prompt: &str) -> Option<String> {
    print!("{}", prompt);
    io::stdout().flush().unwrap();
    
    let mut input = String::new();
    match io::stdin().read_line(&mut input) {
        Ok(0) => None, // EOF reached
        Ok(_) => Some(input.trim().to_string()),
        Err(_) => None,
    }
}

fn get_model_path_from_user() -> String {
    match get_input(&format!("Enter model path (default: {}): ", DEFAULT_MODEL_PATH)) {
        Some(input) if !input.is_empty() => input,
        _ => DEFAULT_MODEL_PATH.to_string(),
    }
}

fn get_context_size_from_user() -> u32 {
    match get_input(&format!("Enter context size (default: {}): ", DEFAULT_CONTEXT_SIZE)) {
        Some(input) if !input.is_empty() => input.parse().unwrap_or(DEFAULT_CONTEXT_SIZE),
        _ => DEFAULT_CONTEXT_SIZE,
    }
}

fn chat(message: &str) {
    println!("AI: You said: {}", message);
    // TODO: Implement actual chat functionality here
}

fn create_conversation_file(model_path: &str, context_size: u32) -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    
    let filename = format!("chat_{}.json", timestamp);
    let filepath = format!("assets/conversations/{}", filename);
    
    // Ensure the directory exists
    if let Some(parent) = Path::new(&filepath).parent() {
        fs::create_dir_all(parent).unwrap();
    }
    
    let conversation = json!({
        "timestamp": timestamp,
        "model_path": model_path,
        "context_size": context_size,
        "messages": []
    });
    
    fs::write(&filepath, serde_json::to_string_pretty(&conversation).unwrap())
        .expect("Failed to create conversation file");
    
    filepath
}

fn add_message(conversation_file: &str, role: &str, message: &str) {
    let content = fs::read_to_string(conversation_file)
        .expect("Failed to read conversation file");
    
    let mut conversation: Value = serde_json::from_str(&content)
        .expect("Failed to parse conversation JSON");
    
    let new_message = json!({
        "role": role,
        "content": message,
        "timestamp": SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
    });
    
    if let Some(messages) = conversation["messages"].as_array_mut() {
        messages.push(new_message);
    }
    
    fs::write(conversation_file, serde_json::to_string_pretty(&conversation).unwrap())
        .expect("Failed to update conversation file");
}

fn add_system_prompt(conversation_file: &str) {
    add_message(conversation_file, "system", SYSTEM_PROMPT);
}