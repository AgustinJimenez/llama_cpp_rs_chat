// Filename pattern matching utilities for model metadata extraction
//
// Uses lookup tables instead of long if-else chains for detecting
// architecture, parameters, and quantization from GGUF filenames.

/// Architecture patterns: (pattern_to_match, display_name)
const ARCHITECTURE_PATTERNS: &[(&str, &str)] = &[
    ("llama", "LLaMA"),
    ("mistral", "Mistral"),
    ("devstral", "Devstral"),
    ("mixtral", "Mixtral"),
    ("qwen", "Qwen"),
    ("gemma", "Gemma"),
    ("phi", "Phi"),
    ("granite", "Granite"),
    ("starcoder", "StarCoder"),
    ("codellama", "CodeLlama"),
    ("deepseek", "DeepSeek"),
    ("yi", "Yi"),
    ("falcon", "Falcon"),
    ("mpt", "MPT"),
    ("bloom", "BLOOM"),
    ("opt", "OPT"),
    ("gpt-j", "GPT-J"),
    ("gpt-neo", "GPT-Neo"),
    ("pythia", "Pythia"),
    ("stablelm", "StableLM"),
    ("vicuna", "Vicuna"),
    ("wizard", "Wizard"),
    ("orca", "Orca"),
    ("openhermes", "OpenHermes"),
    ("neural", "Neural"),
    ("minicpm", "MiniCPM"),
    ("internlm", "InternLM"),
    ("baichuan", "Baichuan"),
    ("chatglm", "ChatGLM"),
    ("command", "Command"),
    ("solar", "Solar"),
    ("nous", "Nous"),
];

/// Parameter size patterns: (pattern_to_match, display_name)
const PARAMETER_PATTERNS: &[(&str, &str)] = &[
    ("405b", "405B"),
    ("180b", "180B"),
    ("141b", "141B"),
    ("123b", "123B"),
    ("110b", "110B"),
    ("72b", "72B"),
    ("70b", "70B"),
    ("65b", "65B"),
    ("46b", "46B"),
    ("34b", "34B"),
    ("33b", "33B"),
    ("30b", "30B"),
    ("27b", "27B"),
    ("22b", "22B"),
    ("20b", "20B"),
    ("14b", "14B"),
    ("13b", "13B"),
    ("12b", "12B"),
    ("11b", "11B"),
    ("10b", "10B"),
    ("9b", "9B"),
    ("8b", "8B"),
    ("7b", "7B"),
    ("6b", "6B"),
    ("4b", "4B"),
    ("3b", "3B"),
    ("2b", "2B"),
    ("1.5b", "1.5B"),
    ("1b", "1B"),
    ("500m", "500M"),
    ("350m", "350M"),
    ("125m", "125M"),
];

/// Quantization patterns: (pattern_to_match, display_name)
const QUANTIZATION_PATTERNS: &[(&str, &str)] = &[
    ("q8_0", "Q8_0"),
    ("q6_k", "Q6_K"),
    ("q5_k_m", "Q5_K_M"),
    ("q5_k_s", "Q5_K_S"),
    ("q5_k", "Q5_K"),
    ("q5_0", "Q5_0"),
    ("q5_1", "Q5_1"),
    ("q4_k_m", "Q4_K_M"),
    ("q4_k_s", "Q4_K_S"),
    ("q4_k", "Q4_K"),
    ("q4_0", "Q4_0"),
    ("q4_1", "Q4_1"),
    ("q3_k_m", "Q3_K_M"),
    ("q3_k_s", "Q3_K_S"),
    ("q3_k_l", "Q3_K_L"),
    ("q3_k", "Q3_K"),
    ("q2_k", "Q2_K"),
    ("iq4_xs", "IQ4_XS"),
    ("iq4_nl", "IQ4_NL"),
    ("iq3_xxs", "IQ3_XXS"),
    ("iq3_xs", "IQ3_XS"),
    ("iq3_s", "IQ3_S"),
    ("iq3_m", "IQ3_M"),
    ("iq2_xxs", "IQ2_XXS"),
    ("iq2_xs", "IQ2_XS"),
    ("iq2_s", "IQ2_S"),
    ("iq1_s", "IQ1_S"),
    ("iq1_m", "IQ1_M"),
    ("fp16", "FP16"),
    ("fp32", "FP32"),
    ("bf16", "BF16"),
    ("f16", "F16"),
    ("f32", "F32"),
];

/// Detect architecture from filename
pub fn detect_architecture(filename: &str) -> &'static str {
    let lower = filename.to_lowercase();
    ARCHITECTURE_PATTERNS
        .iter()
        .find(|(pattern, _)| lower.contains(pattern))
        .map(|(_, name)| *name)
        .unwrap_or("Unknown")
}

/// Detect parameter count from filename
pub fn detect_parameters(filename: &str) -> &'static str {
    let lower = filename.to_lowercase();
    PARAMETER_PATTERNS
        .iter()
        .find(|(pattern, _)| lower.contains(pattern))
        .map(|(_, name)| *name)
        .unwrap_or("Unknown")
}

/// Detect quantization from filename
pub fn detect_quantization(filename: &str) -> &'static str {
    let lower = filename.to_lowercase();
    QUANTIZATION_PATTERNS
        .iter()
        .find(|(pattern, _)| lower.contains(pattern))
        .map(|(_, name)| *name)
        .unwrap_or("Unknown")
}

/// Extract all metadata from filename
#[allow(dead_code)]
pub struct FilenameMetadata {
    pub architecture: &'static str,
    pub parameters: &'static str,
    pub quantization: &'static str,
}

/// Extract all metadata from filename in one call
/// TODO: Use for batch processing of model files
#[allow(dead_code)]
pub fn extract_metadata_from_filename(filename: &str) -> FilenameMetadata {
    FilenameMetadata {
        architecture: detect_architecture(filename),
        parameters: detect_parameters(filename),
        quantization: detect_quantization(filename),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_architecture() {
        assert_eq!(detect_architecture("llama-2-7b-chat.gguf"), "LLaMA");
        assert_eq!(
            detect_architecture("Mistral-7B-Instruct-v0.2.gguf"),
            "Mistral"
        );
        assert_eq!(detect_architecture("Qwen2.5-7B-Instruct.gguf"), "Qwen");
        assert_eq!(detect_architecture("gemma-2-9b-it.gguf"), "Gemma");
        assert_eq!(detect_architecture("unknown-model.gguf"), "Unknown");
    }

    #[test]
    fn test_detect_parameters() {
        assert_eq!(detect_parameters("llama-2-7b-chat.gguf"), "7B");
        assert_eq!(detect_parameters("Mixtral-8x7B.gguf"), "7B");
        assert_eq!(detect_parameters("phi-3-mini-4k-instruct.gguf"), "Unknown");
        assert_eq!(detect_parameters("qwen2.5-72b-instruct.gguf"), "72B");
    }

    #[test]
    fn test_detect_quantization() {
        assert_eq!(detect_quantization("model-Q4_K_M.gguf"), "Q4_K_M");
        assert_eq!(detect_quantization("model-q8_0.gguf"), "Q8_0");
        assert_eq!(detect_quantization("model-IQ4_XS.gguf"), "IQ4_XS");
        assert_eq!(detect_quantization("model-fp16.gguf"), "FP16");
    }

    #[test]
    fn test_extract_metadata() {
        let meta = extract_metadata_from_filename("Qwen2.5-7B-Instruct-Q4_K_M.gguf");
        assert_eq!(meta.architecture, "Qwen");
        assert_eq!(meta.parameters, "7B");
        assert_eq!(meta.quantization, "Q4_K_M");
    }
}
