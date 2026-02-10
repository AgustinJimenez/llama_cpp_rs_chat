//! Per-model tool tag configuration.
//!
//! Different model families are trained with different tool-calling formats.
//! Using a model's native tags in the system prompt makes it far more likely
//! to follow tool-calling instructions than using generic custom tags.

/// Tool tag delimiters used in the system prompt and command detection.
#[derive(Debug, Clone)]
pub struct ToolTags {
    pub exec_open: &'static str,
    pub exec_close: &'static str,
    pub output_open: &'static str,
    pub output_close: &'static str,
}

/// Default SYSTEM.EXEC tags (fallback for unknown models).
pub const DEFAULT_TAGS: ToolTags = ToolTags {
    exec_open: "<||SYSTEM.EXEC>",
    exec_close: "<SYSTEM.EXEC||>",
    output_open: "<||SYSTEM.OUTPUT>",
    output_close: "<SYSTEM.OUTPUT||>",
};

/// Qwen-family native tool tags.
const QWEN_TAGS: ToolTags = ToolTags {
    exec_open: "<tool_call>",
    exec_close: "</tool_call>",
    output_open: "<tool_response>",
    output_close: "</tool_response>",
};

/// Mistral-family native tool tags.
const MISTRAL_TAGS: ToolTags = ToolTags {
    exec_open: "[TOOL_CALLS]",
    exec_close: "[/TOOL_CALLS]",
    output_open: "[TOOL_RESULTS]",
    output_close: "[/TOOL_RESULTS]",
};

/// Harmony-family (gpt-oss-20b) tool tags.
/// Model uses native Harmony format for tool calls (detected by HARMONY_CALL_PATTERN regex).
/// Output tags use Harmony's tool result turn structure.
const HARMONY_TAGS: ToolTags = ToolTags {
    exec_open: "<||SYSTEM.EXEC>",  // Fallback only — model uses native Jinja2 template
    exec_close: "<SYSTEM.EXEC||>",
    output_open: "<|start|>tool<|message|>",
    output_close: "<|end|>",
};

/// Known model name -> tag family mappings.
/// Keyed by `general.name` values from GGUF metadata.
const MODEL_TAG_MAP: &[(&str, &ToolTags)] = &[
    // Qwen models - strong tool calling with native tags
    ("Qwen_Qwen3 Coder Next", &QWEN_TAGS),
    ("Qwen3 8B", &QWEN_TAGS),
    ("Qwen_Qwen3 30B A3B Instruct 2507", &QWEN_TAGS),
    ("Qwen3-Coder-30B-A3B-Instruct-1M", &QWEN_TAGS),
    // Mistral models - strong tool calling with native tags
    ("mistralai_Devstral Small 2507", &MISTRAL_TAGS),
    ("mistralai_Devstral Small 2 24B Instruct 2512", &MISTRAL_TAGS),
    ("Magistral-Small-2509", &MISTRAL_TAGS),
    ("mistralai_Ministral 3 14B Reasoning 2512", &MISTRAL_TAGS),
    // Harmony models - native Harmony format tool calling
    ("Openai_Gpt Oss 20b", &HARMONY_TAGS),
];

/// Normalize a model name for fuzzy matching.
fn normalize(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| if c == '_' || c == '-' { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Look up tool tags for a model by its `general.name` from GGUF metadata.
///
/// Tries exact match first, then fuzzy (normalized substring) match.
/// Returns `DEFAULT_TAGS` if no match is found.
pub fn get_tool_tags_for_model(general_name: Option<&str>) -> &'static ToolTags {
    let name = match general_name {
        Some(n) if !n.is_empty() => n,
        _ => return &DEFAULT_TAGS,
    };

    // Exact match
    for &(key, tags) in MODEL_TAG_MAP {
        if key == name {
            return tags;
        }
    }

    // Fuzzy match
    let normalized = normalize(name);
    for &(key, tags) in MODEL_TAG_MAP {
        let normalized_key = normalize(key);
        if normalized.contains(&normalized_key) || normalized_key.contains(&normalized) {
            return tags;
        }
    }

    &DEFAULT_TAGS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_match_qwen() {
        let tags = get_tool_tags_for_model(Some("Qwen3 8B"));
        assert_eq!(tags.exec_open, "<tool_call>");
    }

    #[test]
    fn test_exact_match_mistral() {
        let tags = get_tool_tags_for_model(Some("mistralai_Devstral Small 2507"));
        assert_eq!(tags.exec_open, "[TOOL_CALLS]");
    }

    #[test]
    fn test_unknown_model_returns_default() {
        let tags = get_tool_tags_for_model(Some("MiniCPM4.1-8B"));
        assert_eq!(tags.exec_open, "<||SYSTEM.EXEC>");
    }

    #[test]
    fn test_none_returns_default() {
        let tags = get_tool_tags_for_model(None);
        assert_eq!(tags.exec_open, "<||SYSTEM.EXEC>");
    }

    #[test]
    fn test_fuzzy_match() {
        // Should fuzzy-match "Qwen3 8B" via normalization
        let tags = get_tool_tags_for_model(Some("qwen3-8b"));
        assert_eq!(tags.exec_open, "<tool_call>");
    }
}
