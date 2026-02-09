use gguf_llms::{GgufHeader, GgufReader, Value};
use std::env;
use std::fs;
use std::io::BufReader;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: print-metadata <path-to-gguf>");
        std::process::exit(1);
    }
    let model_path = &args[1];

    println!("=================================================================");
    println!("Reading GGUF metadata from: {model_path}");
    println!("=================================================================\n");

    let file = match fs::File::open(model_path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Failed to open model file: {e}");
            return;
        }
    };

    let mut reader = BufReader::new(file);

    let header = match GgufHeader::parse(&mut reader) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("Failed to parse GGUF header: {e}");
            return;
        }
    };

    let metadata = match GgufReader::read_metadata(&mut reader, header.n_kv) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("Failed to read metadata: {e}");
            return;
        }
    };

    // Detect architecture
    let arch = metadata
        .get("general.architecture")
        .and_then(|v| if let Value::String(s) = v { Some(s.clone()) } else { None })
        .unwrap_or_else(|| "llama".to_string());

    println!("Architecture: {arch}\n");

    // Helper: get arch-prefixed or llama-prefixed key
    let get = |field: &str| -> Option<&Value> {
        metadata
            .get(&format!("{arch}.{field}"))
            .or_else(|| metadata.get(&format!("llama.{field}")))
    };

    println!("CONTEXT & SIZE INFORMATION:");
    println!("-----------------------------------------------------------------");

    if let Some(v) = get("context_length") {
        match v {
            Value::Uint32(n) => println!("  context_length: {n} tokens"),
            Value::Uint64(n) => println!("  context_length: {n} tokens"),
            _ => println!("  context_length: {v:?}"),
        }
    }
    if let Some(v) = get("embedding_length") {
        println!("  embedding_length: {v:?}");
    }
    if let Some(v) = get("block_count") {
        println!("  block_count (layers): {v:?}");
    }
    if let Some(v) = get("feed_forward_length") {
        println!("  feed_forward_length: {v:?}");
    }

    println!();
    println!("TOKENIZER INFORMATION:");
    println!("-----------------------------------------------------------------");

    for key in [
        "tokenizer.ggml.bos_token_id",
        "tokenizer.ggml.eos_token_id",
        "tokenizer.ggml.model",
    ] {
        if let Some(v) = metadata.get(key) {
            println!("  {key}: {v:?}");
        }
    }

    println!();
    println!("ROPE & ATTENTION INFORMATION:");
    println!("-----------------------------------------------------------------");

    for field in [
        "rope.freq_base",
        "rope.dimension_count",
        "rope.scaling.type",
        "rope.scaling.factor",
        "attention.head_count",
        "attention.head_count_kv",
        "attention.sliding_window",
    ] {
        if let Some(v) = get(field) {
            println!("  {field}: {v:?}");
        }
    }

    println!();
    println!("CHAT TEMPLATE:");
    println!("-----------------------------------------------------------------");

    if let Some(Value::String(template)) = metadata.get("tokenizer.chat_template") {
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

        println!("  Detected type: {template_type}");
        println!("  Template preview (first 200 chars):");
        let preview = if template.len() > 200 {
            format!("{}...", &template[..200])
        } else {
            template.clone()
        };
        println!("    {}", preview.replace('\n', "\n    "));
    } else {
        println!("  No chat template found");
    }

    println!();
    println!("GENERAL MODEL INFO (all general.* keys):");
    println!("-----------------------------------------------------------------");

    let mut general_keys: Vec<_> = metadata
        .keys()
        .filter(|k| k.starts_with("general."))
        .collect();
    general_keys.sort();

    for key in general_keys {
        if let Some(v) = metadata.get(key) {
            println!("  {key}: {v:?}");
        }
    }

    println!();
    println!("SAMPLING PARAMETERS:");
    println!("-----------------------------------------------------------------");

    for (gguf_key, label) in [
        ("general.sampling.temp", "temperature"),
        ("general.sampling.top_p", "top_p"),
        ("general.sampling.top_k", "top_k"),
        ("general.sampling.min_p", "min_p"),
        ("general.sampling.repetition_penalty", "repetition_penalty"),
    ] {
        if let Some(v) = metadata.get(gguf_key) {
            println!("  {label}: {v:?}");
        }
    }

    println!();
    println!("ALL METADATA KEYS (total: {}):", metadata.len());
    println!("-----------------------------------------------------------------");

    let mut keys: Vec<_> = metadata.keys().collect();
    keys.sort();

    for (i, key) in keys.iter().enumerate() {
        if i % 3 == 0 && i > 0 {
            println!();
        }
        print!("  {key:45}");
    }
    println!("\n");

    println!("=================================================================");
    println!("Done!");
    println!("=================================================================");
}
