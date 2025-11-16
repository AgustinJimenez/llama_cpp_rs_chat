use std::fs;
use std::io::BufReader;
use gguf_llms::{GgufHeader, GgufReader, Value};

fn main() {
    let model_path = "E:/.lmstudio/lmstudio-community/gemma-3-12b-it-GGUF/gemma-3-12b-it-Q8_0.gguf";
    let file = fs::File::open(model_path).expect("Failed to open model file");
    let mut reader = BufReader::new(file);
    let header = GgufHeader::parse(&mut reader).expect("Failed to parse GGUF header");
    let metadata = GgufReader::read_metadata(&mut reader, header.n_kv)
        .expect("Failed to read metadata");
    
    println!("=== Gemma-3-12B Context Information ===\n");
    
    // Check for context length
    if let Some(Value::U32(ctx)) = metadata.get("llama.context_length") {
        println!("Max context length: {} tokens", ctx);
    } else if let Some(Value::U64(ctx)) = metadata.get("llama.context_length") {
        println!("Max context length: {} tokens", ctx);
    }
    
    // Check rope settings that might affect context
    if let Some(Value::F32(freq)) = metadata.get("llama.rope.freq_base") {
        println!("RoPE frequency base: {}", freq);
    }
    
    if let Some(Value::U32(train_ctx)) = metadata.get("llama.training_context_length") {
        println!("Training context length: {} tokens", train_ctx);
    }
    
    // List all metadata keys that contain "context" or "length"
    println!("\nAll context-related metadata:");
    for (key, value) in metadata.iter() {
        if key.contains("context") || key.contains("length") || key.contains("rope") {
            println!("  {}: {:?}", key, value);
        }
    }
}
