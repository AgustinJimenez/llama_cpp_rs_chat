use std::io::{self, Write};

fn main() {
    let model_path = get_model_path_from_user();
    let context_size = get_context_size_from_user();
    
    println!("Selected model path: {}", model_path);
    println!("Selected context size: {}", context_size);
    println!("Starting chat session. Type '/exit' to quit.\n");
    
    loop {
        let message = get_input("You: ");
        
        if message == "/exit" {
            println!("Goodbye!");
            break;
        }
        
        chat(&message);
    }
}

fn get_input(prompt: &str) -> String {
    print!("{}", prompt);
    io::stdout().flush().unwrap();
    
    let mut input = String::new();
    io::stdin().read_line(&mut input).expect("Failed to read input");
    input.trim().to_string()
}

fn get_model_path_from_user() -> String {
    let default_path = "E:\\.lmstudio\\models\\lmstudio-community\\Qwen3-Coder-30B-A3B-Instruct-1M-UD-Q8_K_XL-GGUF\\Qwen3-Coder-30B-A3B-Instruct-1M-UD-Q8_K_XL.gguf";
    
    let input = get_input(&format!("Enter model path (default: {}): ", default_path));
    
    if input.is_empty() {
        default_path.to_string()
    } else {
        input
    }
}

fn get_context_size_from_user() -> u32 {
    let default_context = 32000;
    
    let input = get_input(&format!("Enter context size (default: {}): ", default_context));
    
    if input.is_empty() {
        default_context
    } else {
        input.parse().unwrap_or(default_context)
    }
}

fn chat(message: &str) {
    println!("AI: You said: {}", message);
    // TODO: Implement actual chat functionality here
}