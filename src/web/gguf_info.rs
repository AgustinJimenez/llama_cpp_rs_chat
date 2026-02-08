use gguf_llms::{GgufHeader, GgufReader};
use std::io::BufReader;

use super::filename_patterns::{detect_architecture, detect_parameters, detect_quantization};
use super::gguf_utils::{detect_tool_format, extract_default_system_prompt, MetadataExtractor};

/// Extract model information from a GGUF file.
///
/// Reads GGUF metadata (architecture, context length, sampling params, etc.)
/// and combines with filename-based detection for architecture/quantization.
/// Used by the Tauri binary (`main.rs`); not called from the web server binary.
#[allow(dead_code)]
pub fn extract_model_info(decoded_path: &str) -> Result<serde_json::Value, String> {
    const BYTES_PER_GB: u64 = 1_073_741_824;
    const BYTES_PER_MB: u64 = 1_048_576;

    let path_obj = std::path::Path::new(decoded_path);
    if !path_obj.exists() {
        return Err("Model file not found".into());
    }

    if path_obj.is_dir() {
        let mut gguf_files = Vec::new();
        if let Ok(entries) = std::fs::read_dir(path_obj) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_file() {
                    if let Some(ext) = p.extension() {
                        if ext.eq_ignore_ascii_case("gguf") {
                            if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                                gguf_files.push(name.to_string());
                            }
                        }
                    }
                }
            }
        }
        return Err(if gguf_files.is_empty() {
            "This is a directory. No .gguf files found.".into()
        } else {
            format!(
                "This is a directory. Found {} .gguf file(s): {}",
                gguf_files.len(),
                gguf_files.join(", ")
            )
        });
    }

    if let Some(ext) = path_obj.extension() {
        if !ext.eq_ignore_ascii_case("gguf") {
            return Err("File must have .gguf extension".into());
        }
    } else {
        return Err("File must have .gguf extension".into());
    }

    let file_metadata =
        std::fs::metadata(decoded_path).map_err(|_| "Failed to read file metadata".to_string())?;
    let file_size_bytes = file_metadata.len();
    let file_size = if file_size_bytes >= BYTES_PER_GB {
        format!("{:.1} GB", file_size_bytes as f64 / BYTES_PER_GB as f64)
    } else if file_size_bytes >= BYTES_PER_MB {
        format!("{:.1} MB", file_size_bytes as f64 / BYTES_PER_MB as f64)
    } else {
        format!("{file_size_bytes} bytes")
    };

    let filename = path_obj
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    let architecture = detect_architecture(filename);
    let parameters = detect_parameters(filename);
    let quantization = detect_quantization(filename);

    let model_size_gb = file_size_bytes as f64 / BYTES_PER_GB as f64;
    let estimated_total_layers = if model_size_gb < 8.0 {
        36
    } else if model_size_gb < 15.0 {
        45
    } else if model_size_gb < 25.0 {
        60
    } else {
        80
    };

    let mut model_info = serde_json::json!({
        "name": filename,
        "architecture": architecture,
        "parameters": parameters,
        "quantization": quantization,
        "file_size": file_size,
        "context_length": "Variable",
        "path": decoded_path,
        "estimated_layers": estimated_total_layers
    });

    // Try to parse GGUF metadata
    if let Ok(file) = std::fs::File::open(decoded_path) {
        let mut reader = BufReader::new(file);
        if let Ok(header) = GgufHeader::parse(&mut reader) {
            if let Ok(metadata) = GgufReader::read_metadata(&mut reader, header.n_kv) {
                let extractor = MetadataExtractor::new(&metadata);
                model_info["gguf_metadata"] = serde_json::json!(extractor.to_json_map());

                let arch = extractor
                    .get_string("general.architecture")
                    .unwrap_or_else(|| "llama".to_string());
                model_info["architecture"] = serde_json::json!(arch.clone());

                let model_name = extractor.get_string("general.name").unwrap_or_default();
                model_info["tool_format"] = serde_json::json!(detect_tool_format(&arch, &model_name));

                // Core model fields
                for (gguf_key, json_key) in [
                    ("general.name", "general_name"),
                    ("general.author", "author"),
                    ("general.version", "version"),
                    ("general.organization", "organization"),
                    ("general.description", "description"),
                    ("general.license", "license"),
                    ("general.url", "url"),
                    ("general.repo_url", "repo_url"),
                    ("general.file_type", "file_type"),
                    ("general.quantization_version", "quantization_version"),
                ] {
                    if let Some(val) = extractor.get_string(gguf_key) {
                        model_info[json_key] = serde_json::json!(val);
                    }
                }

                // Context length
                for key in [
                    format!("{arch}.context_length"),
                    "llama.context_length".into(),
                    "context_length".into(),
                ] {
                    if let Some(val) = extractor.get_string(&key) {
                        model_info["context_length"] = serde_json::json!(val);
                        break;
                    }
                }

                // Architecture-specific fields
                for (field, json_key) in [
                    ("embedding_length", "embedding_length"),
                    ("feed_forward_length", "feed_forward_length"),
                    ("attention.head_count", "attention_head_count"),
                    ("attention.head_count_kv", "attention_head_count_kv"),
                    ("attention.layer_norm_rms_epsilon", "layer_norm_epsilon"),
                    ("rope.dimension_count", "rope_dimension_count"),
                    ("rope.freq_base", "rope_freq_base"),
                ] {
                    if let Some(val) = extractor.get_arch_field(&arch, field) {
                        model_info[json_key] = serde_json::json!(val);
                    }
                }

                // Block count (also used for layer estimation)
                if let Some(val) = extractor.get_arch_field(&arch, "block_count") {
                    model_info["block_count"] = serde_json::json!(val.clone());
                    if let Ok(count) = val.parse::<u32>() {
                        model_info["estimated_layers"] = serde_json::json!(count);
                    }
                }

                // Tokenizer
                for (gguf_key, json_key) in [
                    ("tokenizer.ggml.model", "tokenizer_model"),
                    ("tokenizer.ggml.bos_token_id", "bos_token_id"),
                    ("tokenizer.ggml.eos_token_id", "eos_token_id"),
                    ("tokenizer.ggml.padding_token_id", "padding_token_id"),
                ] {
                    if let Some(val) = extractor.get_string(gguf_key) {
                        model_info[json_key] = serde_json::json!(val);
                    }
                }

                // Chat template
                if let Some(val) = extractor.get_string("tokenizer.chat_template") {
                    model_info["chat_template"] = serde_json::json!(val.clone());
                    if let Some(prompt) = extract_default_system_prompt(&val) {
                        model_info["default_system_prompt"] = serde_json::json!(prompt);
                    }
                }

                // GGUF embedded sampling parameters
                let mut recommended = serde_json::Map::new();
                for (gguf_key, param_name) in [
                    ("general.sampling.temp", "temperature"),
                    ("general.sampling.top_p", "top_p"),
                    ("general.sampling.top_k", "top_k"),
                    ("general.sampling.min_p", "min_p"),
                    ("general.sampling.repetition_penalty", "repetition_penalty"),
                ] {
                    if let Some(val) = extractor.get_json(gguf_key) {
                        recommended.insert(param_name.to_string(), val);
                    }
                }
                if !recommended.is_empty() {
                    model_info["recommended_params"] = serde_json::json!(recommended);
                }
            }
        }
    }

    Ok(model_info)
}
