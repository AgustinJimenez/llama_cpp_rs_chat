// Test program to explore llama-cpp-2 chat template API
use llama_cpp_2::{
    llama_backend::LlamaBackend,
    model::{params::LlamaModelParams, LlamaModel, chat_template::*},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Testing llama-cpp-2 chat template API...");

    // Try to create a chat message
    let message = LlamaChatMessage::new("user".to_string(), "Hello".to_string())?;

    println!("Created message: {:?}", message);

    Ok(())
}
