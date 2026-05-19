use std::fs;

use llama_chat_engine::{get_tool_tags_for_model, tool_tags::get_tag_pairs_for_model};
use llama_chat_engine::gguf_utils::{
    detect_tool_format, extract_default_system_prompt, MetadataExtractor,
};

pub(super) fn default_model_status_json() -> String {
    r#"{"loaded":false,"model_path":null,"last_used":null,"memory_usage_mb":null}"#.to_string()
}

pub(super) fn scan_directory_for_gguf_files(path: &std::path::Path) -> Vec<String> {
    let mut gguf_files = Vec::new();
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let entry_path = entry.path();
            if entry_path.is_file() {
                if let Some(ext) = entry_path.extension() {
                    if ext.eq_ignore_ascii_case("gguf") {
                        if let Some(filename) = entry_path.file_name().and_then(|n| n.to_str()) {
                            gguf_files.push(filename.to_string());
                        }
                    }
                }
            }
        }
    }
    gguf_files
}

pub(super) fn scan_for_mmproj_files(model_path: &std::path::Path) -> Vec<(String, u64)> {
    let dir = match model_path.parent() {
        Some(d) => d,
        None => return Vec::new(),
    };
    let mut results = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let entry_path = entry.path();
            if !entry_path.is_file() {
                continue;
            }
            let ext_ok = entry_path
                .extension()
                .map(|e| e.eq_ignore_ascii_case("gguf"))
                .unwrap_or(false);
            if !ext_ok {
                continue;
            }
            if let Some(name) = entry_path.file_name().and_then(|n| n.to_str()) {
                let lower = name.to_ascii_lowercase();
                if lower.contains("mmproj") {
                    let size = fs::metadata(&entry_path).map(|m| m.len()).unwrap_or(0);
                    results.push((name.to_string(), size));
                }
            }
        }
    }
    results
}

pub(super) fn enrich_model_info_from_gguf(
    model_info: &mut serde_json::Value,
    extractor: &MetadataExtractor,
) {
    model_info["gguf_metadata"] = serde_json::json!(extractor.to_json_map());
    let arch = extractor
        .get_string("general.architecture")
        .unwrap_or_else(|| "llama".to_string());
    model_info["architecture"] = serde_json::json!(arch.clone());

    let model_name = extractor.get_string("general.name").unwrap_or_default();
    model_info["tool_format"] = serde_json::json!(detect_tool_format(&arch, &model_name));
    let detected_tags = get_tool_tags_for_model(Some(&model_name));
    model_info["detected_tool_tags"] = serde_json::json!({
        "exec_open": detected_tags.exec_open,
        "exec_close": detected_tags.exec_close,
        "output_open": detected_tags.output_open,
        "output_close": detected_tags.output_close,
    });
    model_info["detected_tag_pairs"] = serde_json::json!(get_tag_pairs_for_model(Some(&model_name)));

    let string_fields = [
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
        ("tokenizer.ggml.model", "tokenizer_model"),
        ("tokenizer.ggml.bos_token_id", "bos_token_id"),
        ("tokenizer.ggml.eos_token_id", "eos_token_id"),
        ("tokenizer.ggml.padding_token_id", "padding_token_id"),
    ];
    for (key, field) in &string_fields {
        if let Some(val) = extractor.get_string(key) {
            model_info[*field] = serde_json::json!(val);
        }
    }

    for key in &[
        format!("{arch}.context_length"),
        "llama.context_length".to_string(),
        "context_length".to_string(),
    ] {
        if let Some(val) = extractor.get_string(key) {
            model_info["context_length"] = serde_json::json!(val);
            break;
        }
    }

    let arch_fields = [
        ("embedding_length", "embedding_length"),
        ("feed_forward_length", "feed_forward_length"),
        ("attention.head_count", "attention_head_count"),
        ("attention.head_count_kv", "attention_head_count_kv"),
        ("attention.layer_norm_rms_epsilon", "layer_norm_epsilon"),
        ("rope.dimension_count", "rope_dimension_count"),
        ("rope.freq_base", "rope_freq_base"),
    ];
    for (suffix, field) in &arch_fields {
        if let Some(val) = extractor.get_arch_field(&arch, suffix) {
            model_info[*field] = serde_json::json!(val);
        }
    }
    if let Some(val) = extractor.get_arch_field(&arch, "block_count") {
        model_info["block_count"] = serde_json::json!(val.clone());
        if let Ok(count) = val.parse::<u32>() {
            model_info["estimated_layers"] = serde_json::json!(count);
        }
    }

    if let Some(val) = extractor.get_string("tokenizer.chat_template") {
        model_info["chat_template"] = serde_json::json!(val.clone());
        if let Some(prompt) = extract_default_system_prompt(&val) {
            model_info["default_system_prompt"] = serde_json::json!(prompt);
        }
    }

    let sampling_fields = [
        ("general.sampling.temp", "temperature"),
        ("general.sampling.top_p", "top_p"),
        ("general.sampling.top_k", "top_k"),
        ("general.sampling.min_p", "min_p"),
        ("general.sampling.repetition_penalty", "repetition_penalty"),
    ];
    let mut recommended_params = serde_json::Map::new();
    for (key, name) in &sampling_fields {
        if let Some(val) = extractor.get_json(key) {
            recommended_params.insert((*name).to_string(), val);
        }
    }
    if !recommended_params.is_empty() {
        model_info["recommended_params"] = serde_json::json!(recommended_params);
    }
}

pub(super) fn detect_nvidia_gpu_hardware() -> bool {
    #[cfg(target_os = "windows")]
    {
        std::path::Path::new("C:\\Windows\\System32\\nvcuda.dll").exists()
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::process::Command::new("nvidia-smi")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}
