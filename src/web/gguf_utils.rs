// GGUF metadata utilities - shared across lib.rs and routes/model.rs
//
// This module provides a clean interface for reading GGUF file metadata
// using the gguf_llms crate.

use std::fs::File;
use std::io::BufReader;
use std::collections::HashMap;
use gguf_llms::{GgufHeader, GgufReader, Value};

/// Basic model metadata extracted from GGUF file
/// TODO: Use for caching model metadata to avoid repeated GGUF file reads
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct GgufBasicMetadata {
    pub architecture: String,
    pub parameters: String,
    pub quantization: String,
    pub context_length: String,
}

/// Convert a GGUF Value to an Option<String>
pub fn value_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Uint8(n) => Some(n.to_string()),
        Value::Uint16(n) => Some(n.to_string()),
        Value::Uint32(n) => Some(n.to_string()),
        Value::Uint64(n) => Some(n.to_string()),
        Value::Int8(n) => Some(n.to_string()),
        Value::Int16(n) => Some(n.to_string()),
        Value::Int32(n) => Some(n.to_string()),
        Value::Int64(n) => Some(n.to_string()),
        Value::Float32(f) => Some(f.to_string()),
        Value::Float64(f) => Some(f.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Array(_, _) => None,
    }
}

/// Convert a GGUF Value to a serde_json::Value
pub fn value_to_json(value: &Value) -> serde_json::Value {
    match value {
        Value::String(s) => serde_json::json!(s),
        Value::Uint8(n) => serde_json::json!(n),
        Value::Uint16(n) => serde_json::json!(n),
        Value::Uint32(n) => serde_json::json!(n),
        Value::Uint64(n) => serde_json::json!(n),
        Value::Int8(n) => serde_json::json!(n),
        Value::Int16(n) => serde_json::json!(n),
        Value::Int32(n) => serde_json::json!(n),
        Value::Int64(n) => serde_json::json!(n),
        Value::Float32(f) => serde_json::json!(f),
        Value::Float64(f) => serde_json::json!(f),
        Value::Bool(b) => serde_json::json!(b),
        Value::Array(_, items) => serde_json::json!(format!("[Array with {} items]", items.len())),
    }
}

/// Convert a GGUF Value to a display string (for debugging/logging)
pub fn value_to_display_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Uint8(n) => n.to_string(),
        Value::Uint16(n) => n.to_string(),
        Value::Uint32(n) => n.to_string(),
        Value::Uint64(n) => n.to_string(),
        Value::Int8(n) => n.to_string(),
        Value::Int16(n) => n.to_string(),
        Value::Int32(n) => n.to_string(),
        Value::Int64(n) => n.to_string(),
        Value::Float32(f) => f.to_string(),
        Value::Float64(f) => f.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Array(_, items) => format!("[Array with {} items]", items.len()),
    }
}

/// Format parameter count to human-readable string (e.g., "7B", "13B")
/// TODO: Use in UI to display model sizes in human-readable format
#[allow(dead_code)]
pub fn format_parameter_count(param_str: &str) -> String {
    if let Ok(count) = param_str.parse::<u64>() {
        if count >= 1_000_000_000 {
            format!("{}B", count / 1_000_000_000)
        } else if count >= 1_000_000 {
            format!("{}M", count / 1_000_000)
        } else {
            count.to_string()
        }
    } else {
        param_str.to_string()
    }
}

/// Read GGUF metadata from a file using gguf_llms crate.
/// Returns the raw metadata HashMap for full access.
/// TODO: Use for API endpoint that returns full model metadata
#[allow(dead_code)]
pub fn read_gguf_metadata_raw(file_path: &str) -> Result<HashMap<String, Value>, String> {
    let file = File::open(file_path)
        .map_err(|e| format!("Failed to open file: {}", e))?;

    let mut reader = BufReader::new(file);

    let header = GgufHeader::parse(&mut reader)
        .map_err(|e| format!("Failed to parse GGUF header: {}", e))?;

    let metadata = GgufReader::read_metadata(&mut reader, header.n_kv)
        .map_err(|e| format!("Failed to read GGUF metadata: {}", e))?;

    Ok(metadata)
}

/// Read basic metadata from GGUF file (architecture, parameters, quantization, context_length).
/// This is a convenience function for simple use cases.
/// TODO: Use for quick model info display without full metadata extraction
#[allow(dead_code)]
pub fn read_gguf_basic_metadata(file_path: &str) -> Result<GgufBasicMetadata, String> {
    let metadata = read_gguf_metadata_raw(file_path)?;

    // Helper closure to get metadata value as string
    let get_string = |key: &str| -> Option<String> {
        metadata.get(key).and_then(value_to_string)
    };

    // Get architecture
    let architecture = get_string("general.architecture")
        .or_else(|| get_string("general.arch"))
        .unwrap_or_else(|| "Unknown".to_string());

    // Get parameters with formatting
    let parameters = get_string("general.parameter_count")
        .or_else(|| get_string("general.param_count"))
        .map(|p| format_parameter_count(&p))
        .unwrap_or_else(|| "Unknown".to_string());

    // Get quantization
    let quantization = get_string("general.quantization_version")
        .or_else(|| get_string("general.file_type"))
        .unwrap_or_else(|| "Unknown".to_string());

    // Get context length - try architecture-specific key first
    let context_length = get_string(&format!("{}.context_length", architecture))
        .or_else(|| get_string("llama.context_length"))
        .or_else(|| get_string("general.context_length"))
        .or_else(|| get_string("context_length"))
        .unwrap_or_else(|| "Unknown".to_string());

    Ok(GgufBasicMetadata {
        architecture,
        parameters,
        quantization,
        context_length,
    })
}

/// Extract default system prompt from chat template if present.
pub fn extract_default_system_prompt(chat_template: &str) -> Option<String> {
    // Look for: {%- set default_system_message = '...' %}
    if let Some(start_idx) = chat_template.find("set default_system_message = '") {
        let after_start = &chat_template[start_idx + "set default_system_message = '".len()..];
        if let Some(end_idx) = after_start.find("' %}") {
            return Some(after_start[..end_idx].to_string());
        }
    }
    None
}

/// Detect tool calling format based on architecture and model name.
pub fn detect_tool_format(architecture: &str, model_name: &str) -> &'static str {
    let arch_lower = architecture.to_lowercase();
    let name_lower = model_name.to_lowercase();

    if arch_lower.contains("mistral") || name_lower.contains("mistral") || name_lower.contains("devstral") {
        "mistral"
    } else if arch_lower.contains("llama") && (name_lower.contains("llama-3") || name_lower.contains("llama3")) {
        "llama3"
    } else if arch_lower.contains("qwen") || name_lower.contains("qwen") {
        "qwen"
    } else if arch_lower.contains("llama") {
        // Older llama models don't support tools
        "unknown"
    } else {
        "unknown"
    }
}

/// Parse model filename to extract architecture, parameters, and quantization.
/// This is a fallback when GGUF metadata parsing fails.
/// TODO: Use as fallback when GGUF file cannot be read
#[allow(dead_code)]
pub fn parse_model_filename(filename: &str) -> (String, String, String) {
    let lower = filename.to_lowercase();

    // Extract architecture
    let architecture = if lower.contains("llama") {
        "LLaMA"
    } else if lower.contains("mistral") {
        "Mistral"
    } else if lower.contains("qwen") {
        "Qwen"
    } else if lower.contains("granite") {
        "Granite"
    } else if lower.contains("codellama") {
        "Code Llama"
    } else if lower.contains("phi") {
        "Phi"
    } else if lower.contains("gemma") {
        "Gemma"
    } else if lower.contains("falcon") {
        "Falcon"
    } else if lower.contains("vicuna") {
        "Vicuna"
    } else if lower.contains("deepseek") {
        "DeepSeek"
    } else {
        "Unknown"
    }.to_string();

    // Extract parameters (look for patterns like 7B, 13B, 70B)
    let parameters = extract_param_count(&lower);

    // Extract quantization (look for patterns like Q4_K_M, Q8_0, etc.)
    let quantization = extract_quantization(&lower);

    (architecture, parameters, quantization)
}

fn extract_param_count(filename: &str) -> String {
    // Common patterns: 7b, 13b, 70b, 7B, 13B, etc.
    let patterns = [
        "405b", "236b", "180b", "141b", "123b", "110b", "90b", "80b", "72b", "70b",
        "65b", "46b", "40b", "35b", "34b", "32b", "30b", "27b", "22b", "20b", "14b",
        "13b", "12b", "11b", "8b", "7b", "6b", "4b", "3b", "2b", "1b",
        "0.5b", "1.8b", "2.8b", "3.8b", "4.5b",
    ];

    for pattern in patterns {
        if filename.contains(pattern) {
            return pattern.to_uppercase();
        }
    }

    "Unknown".to_string()
}

fn extract_quantization(filename: &str) -> String {
    // Common quantization patterns
    let patterns = [
        "q8_0", "q6_k", "q5_k_m", "q5_k_s", "q5_1", "q5_0",
        "q4_k_m", "q4_k_s", "q4_1", "q4_0", "q3_k_m", "q3_k_s",
        "q2_k", "iq4_xs", "iq3_xxs", "iq2_xxs", "f16", "f32", "bf16",
    ];

    for pattern in patterns {
        if filename.contains(pattern) {
            return pattern.to_uppercase();
        }
    }

    "Unknown".to_string()
}

/// Helper struct for creating metadata extractors
pub struct MetadataExtractor<'a> {
    metadata: &'a HashMap<String, Value>,
}

impl<'a> MetadataExtractor<'a> {
    pub fn new(metadata: &'a HashMap<String, Value>) -> Self {
        Self { metadata }
    }

    /// Get a metadata value as Option<String>
    pub fn get_string(&self, key: &str) -> Option<String> {
        self.metadata.get(key).and_then(value_to_string)
    }

    /// Get a metadata value as serde_json::Value
    pub fn get_json(&self, key: &str) -> Option<serde_json::Value> {
        self.metadata.get(key).map(value_to_json)
    }

    /// Get architecture-specific field
    pub fn get_arch_field(&self, arch: &str, field: &str) -> Option<String> {
        self.get_string(&format!("{}.{}", arch, field))
    }

    /// Convert all metadata to a JSON map
    pub fn to_json_map(&self) -> serde_json::Map<String, serde_json::Value> {
        let mut map = serde_json::Map::new();
        for (key, value) in self.metadata.iter() {
            map.insert(key.clone(), value_to_json(value));
        }
        map
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_parameter_count_billions() {
        assert_eq!(format_parameter_count("7000000000"), "7B");
        assert_eq!(format_parameter_count("13000000000"), "13B");
        assert_eq!(format_parameter_count("70000000000"), "70B");
    }

    #[test]
    fn test_format_parameter_count_millions() {
        assert_eq!(format_parameter_count("500000000"), "500M");
        assert_eq!(format_parameter_count("125000000"), "125M");
    }

    #[test]
    fn test_format_parameter_count_invalid() {
        assert_eq!(format_parameter_count("unknown"), "unknown");
        assert_eq!(format_parameter_count("7B"), "7B");
    }

    #[test]
    fn test_parse_model_filename_architecture() {
        let (arch, _, _) = parse_model_filename("llama-2-7b-chat.gguf");
        assert_eq!(arch, "LLaMA");

        let (arch, _, _) = parse_model_filename("mistral-7b-instruct.gguf");
        assert_eq!(arch, "Mistral");

        let (arch, _, _) = parse_model_filename("qwen2-7b.gguf");
        assert_eq!(arch, "Qwen");
    }

    #[test]
    fn test_parse_model_filename_params() {
        let (_, params, _) = parse_model_filename("llama-2-7b-chat.gguf");
        assert_eq!(params, "7B");

        let (_, params, _) = parse_model_filename("llama-2-70b.gguf");
        assert_eq!(params, "70B");
    }

    #[test]
    fn test_parse_model_filename_quantization() {
        let (_, _, quant) = parse_model_filename("model-q4_k_m.gguf");
        assert_eq!(quant, "Q4_K_M");

        let (_, _, quant) = parse_model_filename("model-q8_0.gguf");
        assert_eq!(quant, "Q8_0");
    }

    #[test]
    fn test_detect_tool_format() {
        assert_eq!(detect_tool_format("mistral", "mistral-7b"), "mistral");
        assert_eq!(detect_tool_format("llama", "llama-3-8b"), "llama3");
        assert_eq!(detect_tool_format("qwen2", "qwen2-7b"), "qwen");
        assert_eq!(detect_tool_format("llama", "llama-2-7b"), "unknown");
    }

    #[test]
    fn test_extract_default_system_prompt() {
        let template = "{%- set default_system_message = 'You are a helpful assistant.' %} rest of template";
        assert_eq!(
            extract_default_system_prompt(template),
            Some("You are a helpful assistant.".to_string())
        );

        let template_no_prompt = "no system prompt here";
        assert_eq!(extract_default_system_prompt(template_no_prompt), None);
    }
}
