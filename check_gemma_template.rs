use gguf_llms::{GgufHeader, GgufReader, Value};
use std::fs;
use std::io::BufReader;

fn main() {
    let model_path = "E:/.lmstudio/lmstudio-community/gemma-3-12b-it-GGUF/gemma-3-12b-it-Q8_0.gguf";

    if let Ok(file) = fs::File::open(model_path) {
        let mut reader = BufReader::new(file);
        if let Ok(header) = GgufHeader::parse(&mut reader) {
            if let Ok(metadata) = GgufReader::read_metadata(&mut reader, header.n_kv) {
                println!("=== Gemma Model GGUF Metadata ===\n");

                // Print chat template
                if let Some(Value::String(template)) = metadata.get("tokenizer.chat_template") {
                    println!("Chat template:");
                    println!("{}\n", template);
                    println!("Template length: {} chars\n", template.len());
                }

                // Print model name
                if let Some(Value::String(name)) = metadata.get("general.name") {
                    println!("Model name: {}", name);
                }

                // Print architecture
                if let Some(Value::String(arch)) = metadata.get("general.architecture") {
                    println!("Architecture: {}", arch);
                }

                // Print context length
                if let Some(ctx) = metadata.get("llama.context_length") {
                    match ctx {
                        Value::Uint32(n) => println!("Context length: {}", n),
                        Value::Uint64(n) => println!("Context length: {}", n),
                        _ => {}
                    }
                }

                // Print all keys for debugging
                println!("\n=== All Metadata Keys ===");
                let mut keys: Vec<_> = metadata.keys().collect();
                keys.sort();
                for key in keys {
                    println!("  - {}", key);
                }
            }
        }
    } else {
        eprintln!("Failed to open model file: {}", model_path);
    }
}
