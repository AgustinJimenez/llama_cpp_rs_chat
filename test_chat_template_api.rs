use llama_cpp_2::{
    context::{params::LlamaContextParams, LlamaContext},
    llama_backend::LlamaBackend,
    model::{params::LlamaModelParams, LlamaModel},
};

fn main() {
    println!("Testing llama-cpp-2 chat template API...");
    
    // Initialize backend
    let backend = LlamaBackend::init().unwrap();
    
    // Check if we can find methods related to chat templates
    // This is a compile test to see what methods are available
    
    // We can't actually load a model without a file, but we can check the API
    // Let's see what methods exist on LlamaModel and LlamaContext
    
    println!("Available methods (check compilation errors for hints):");
    
    // Uncomment these lines one by one to see what methods exist:
    
    // Check if apply_chat_template exists
    // model.apply_chat_template();
    
    // Check if format_chat exists  
    // model.format_chat();
    
    // Check if there's a chat_template method
    // model.chat_template();
    
    // Check context methods
    // context.apply_chat_template();
    
    println!("Test complete - check compiler output for available methods");
}