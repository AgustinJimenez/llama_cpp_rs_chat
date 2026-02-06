use gguf_llms::{GgufHeader, GgufReader, Value};
use std::fs;
use std::io::BufReader;

fn main() {
    let model_path = "E:/.lmstudio/lmstudio-community/gemma-3-12b-it-GGUF/gemma-3-12b-it-Q8_0.gguf";

    println!("=================================================================");
    println!("Reading GGUF metadata from: {}", model_path);
    println!("=================================================================\n");

    let file = match fs::File::open(model_path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Failed to open model file: {}", e);
            return;
        }
    };

    let mut reader = BufReader::new(file);

    let header = match GgufHeader::parse(&mut reader) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("Failed to parse GGUF header: {}", e);
            return;
        }
    };

    let metadata = match GgufReader::read_metadata(&mut reader, header.n_kv) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("Failed to read metadata: {}", e);
            return;
        }
    };

    println!("📊 CONTEXT & SIZE INFORMATION:");
    println!("-----------------------------------------------------------------");

    // Context length - try both llama and gemma3 prefixes
    if let Some(v) = metadata
        .get("gemma3.context_length")
        .or_else(|| metadata.get("llama.context_length"))
    {
        match v {
            Value::Uint32(n) => println!("  context_length: {} tokens", n),
            Value::Uint64(n) => println!("  context_length: {} tokens", n),
            _ => println!("  context_length: {:?}", v),
        }
    }

    // Embedding length
    if let Some(v) = metadata
        .get("gemma3.embedding_length")
        .or_else(|| metadata.get("llama.embedding_length"))
    {
        println!("  embedding_length: {:?}", v);
    }

    // Block count (layers)
    if let Some(v) = metadata
        .get("gemma3.block_count")
        .or_else(|| metadata.get("llama.block_count"))
    {
        println!("  block_count (layers): {:?}", v);
    }

    // Feed forward length
    if let Some(v) = metadata
        .get("gemma3.feed_forward_length")
        .or_else(|| metadata.get("llama.feed_forward_length"))
    {
        println!("  feed_forward_length: {:?}", v);
    }

    println!();
    println!("🔤 TOKENIZER INFORMATION:");
    println!("-----------------------------------------------------------------");

    // Token IDs
    if let Some(v) = metadata.get("tokenizer.ggml.bos_token_id") {
        println!("  tokenizer.ggml.bos_token_id: {:?}", v);
    }
    if let Some(v) = metadata.get("tokenizer.ggml.eos_token_id") {
        println!("  tokenizer.ggml.eos_token_id: {:?}", v);
    }
    if let Some(v) = metadata.get("tokenizer.ggml.model") {
        println!("  tokenizer.ggml.model: {:?}", v);
    }

    println!();
    println!("🧬 ROPE & ATTENTION INFORMATION:");
    println!("-----------------------------------------------------------------");

    // RoPE settings (affect context extension)
    if let Some(v) = metadata
        .get("gemma3.rope.freq_base")
        .or_else(|| metadata.get("llama.rope.freq_base"))
    {
        println!("  rope.freq_base: {:?}", v);
    }
    if let Some(v) = metadata
        .get("gemma3.rope.dimension_count")
        .or_else(|| metadata.get("llama.rope.dimension_count"))
    {
        println!("  rope.dimension_count: {:?}", v);
    }
    if let Some(v) = metadata
        .get("gemma3.rope.scaling.type")
        .or_else(|| metadata.get("llama.rope.scaling.type"))
    {
        println!("  rope.scaling.type: {:?}", v);
    }
    if let Some(v) = metadata
        .get("gemma3.rope.scaling.factor")
        .or_else(|| metadata.get("llama.rope.scaling.factor"))
    {
        println!("  rope.scaling.factor: {:?}", v);
    }

    // Attention
    if let Some(v) = metadata
        .get("gemma3.attention.head_count")
        .or_else(|| metadata.get("llama.attention.head_count"))
    {
        println!("  attention.head_count: {:?}", v);
    }
    if let Some(v) = metadata
        .get("gemma3.attention.head_count_kv")
        .or_else(|| metadata.get("llama.attention.head_count_kv"))
    {
        println!("  attention.head_count_kv: {:?}", v);
    }
    if let Some(v) = metadata.get("gemma3.attention.sliding_window") {
        println!("  attention.sliding_window: {:?}", v);
    }

    println!();
    println!("💬 CHAT TEMPLATE:");
    println!("-----------------------------------------------------------------");

    if let Some(Value::String(template)) = metadata.get("tokenizer.chat_template") {
        // Detect template type
        let template_type = if template.contains("<|im_start|>") && template.contains("<|im_end|>")
        {
            "ChatML (Qwen, OpenAI)"
        } else if template.contains("[INST]") && template.contains("[/INST]") {
            "Mistral"
        } else if template.contains("<|start_header_id|>") {
            "Llama3"
        } else if template.contains("<start_of_turn>") && template.contains("<end_of_turn>") {
            "Gemma"
        } else {
            "Unknown/Generic"
        };

        println!("  Detected type: {}", template_type);
        println!("  Template preview (first 200 chars):");
        let preview = if template.len() > 200 {
            format!("{}...", &template[..200])
        } else {
            template.clone()
        };
        println!("    {}", preview.replace("\n", "\n    "));
    } else {
        println!("  No chat template found");
    }

    println!();
    println!("🎯 GENERAL MODEL INFO:");
    println!("-----------------------------------------------------------------");

    if let Some(v) = metadata.get("general.architecture") {
        println!("  general.architecture: {:?}", v);
    }
    if let Some(v) = metadata.get("general.name") {
        println!("  general.name: {:?}", v);
    }
    if let Some(v) = metadata.get("general.file_type") {
        println!("  general.file_type: {:?}", v);
    }

    println!();
    println!("📋 ALL METADATA KEYS (total: {}):", metadata.len());
    println!("-----------------------------------------------------------------");

    let mut keys: Vec<_> = metadata.keys().collect();
    keys.sort();

    for (i, key) in keys.iter().enumerate() {
        if i % 3 == 0 && i > 0 {
            println!();
        }
        print!("  {:40}", key);
        if (i + 1) % 3 == 0 {
            println!();
        }
    }
    println!("\n");

    println!("=================================================================");
    println!("Done!");
    println!("=================================================================");
}
