//! Per-model tool tag configuration.
//!
//! Different model families are trained with different tool-calling formats.
//! Using a model's native tags in the system prompt makes it far more likely
//! to follow tool-calling instructions than using generic custom tags.

/// Tool tag delimiters used in the system prompt and command detection.
#[derive(Debug, Clone)]
pub struct ToolTags {
    pub exec_open: String,
    pub exec_close: String,
    pub output_open: String,
    pub output_close: String,
}

impl ToolTags {
    fn new(exec_open: &str, exec_close: &str, output_open: &str, output_close: &str) -> Self {
        Self {
            exec_open: exec_open.to_string(),
            exec_close: exec_close.to_string(),
            output_open: output_open.to_string(),
            output_close: output_close.to_string(),
        }
    }

    /// Apply user overrides. Non-empty values replace the auto-detected defaults.
    pub fn with_overrides(
        mut self,
        exec_open: Option<&str>,
        exec_close: Option<&str>,
        output_open: Option<&str>,
        output_close: Option<&str>,
    ) -> Self {
        if let Some(v) = exec_open {
            if !v.is_empty() {
                self.exec_open = v.to_string();
            }
        }
        if let Some(v) = exec_close {
            if !v.is_empty() {
                self.exec_close = v.to_string();
            }
        }
        if let Some(v) = output_open {
            if !v.is_empty() {
                self.output_open = v.to_string();
            }
        }
        if let Some(v) = output_close {
            if !v.is_empty() {
                self.output_close = v.to_string();
            }
        }
        self
    }
}

/// Default SYSTEM.EXEC tags (fallback for unknown models).
pub fn default_tags() -> ToolTags {
    ToolTags::new(
        "<||SYSTEM.EXEC>",
        "<SYSTEM.EXEC||>",
        "<||SYSTEM.OUTPUT>",
        "<SYSTEM.OUTPUT||>",
    )
}

/// Qwen-family native tool tags.
fn qwen_tags() -> ToolTags {
    ToolTags::new(
        "<tool_call>",
        "</tool_call>",
        "<tool_response>",
        "</tool_response>",
    )
}

/// Mistral-family native tool tags.
fn mistral_tags() -> ToolTags {
    ToolTags::new(
        "[TOOL_CALLS]",
        "[/TOOL_CALLS]",
        "[TOOL_RESULTS]",
        "[/TOOL_RESULTS]",
    )
}

/// Harmony-family (gpt-oss-20b) tool tags.
/// Model uses native Harmony format for tool calls (detected by HARMONY_CALL_PATTERN regex).
/// Output tags use Harmony's tool result turn structure.
fn harmony_tags() -> ToolTags {
    ToolTags::new(
        "<||SYSTEM.EXEC>", // Fallback only — model uses native Jinja2 template
        "<SYSTEM.EXEC||>",
        "<|start|>tool<|message|>",
        "<|end|>",
    )
}

/// GLM-family native tool tags.
/// GLM uses `<tool_call>`/`</tool_call>` for calls and `<|observation|>` for results.
/// The output_close is empty because `wrap_output_for_model()` adds `<|assistant|>`
/// for model injection to re-open the assistant turn.
#[allow(dead_code)]
fn glm_tags() -> ToolTags {
    ToolTags::new("<tool_call>", "</tool_call>", "<|observation|>", "")
}

/// Tag factory function type for the model map.
type TagFactory = fn() -> ToolTags;

/// Known model name -> tag family mappings.
/// Keyed by `general.name` values from GGUF metadata.
const MODEL_TAG_MAP: &[(&str, TagFactory)] = &[
    // Qwen models - strong tool calling with native tags
    ("Qwen_Qwen3 Coder Next", qwen_tags),
    ("Qwen3 8B", qwen_tags),
    ("Qwen_Qwen3 30B A3B Instruct 2507", qwen_tags),
    ("Qwen3-Coder-30B-A3B-Instruct-1M", qwen_tags),
    // Mistral models - strong tool calling with native tags
    ("mistralai_Devstral Small 2507", mistral_tags),
    ("mistralai_Devstral Small 2 24B Instruct 2512", mistral_tags),
    ("Magistral-Small-2509", mistral_tags),
    ("mistralai_Ministral 3 14B Reasoning 2512", mistral_tags),
    // Harmony models - native Harmony format tool calling
    ("Openai_Gpt Oss 20b", harmony_tags),
    // GLM models - use default SYSTEM.EXEC tags (model doesn't close <tool_call> properly)
    // glm_tags kept for reference but not used - model generates <|end_of_box|> instead of </tool_call>
    ("Zai org_GLM 4.6V Flash", default_tags),
    ("Zai org_GLM 4.7 Flash", default_tags),
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
/// Returns default tags if no match is found.
pub fn get_tool_tags_for_model(general_name: Option<&str>) -> ToolTags {
    let name = match general_name {
        Some(n) if !n.is_empty() => n,
        _ => return default_tags(),
    };

    // Exact match
    for &(key, factory) in MODEL_TAG_MAP {
        if key == name {
            return factory();
        }
    }

    // Fuzzy match
    let normalized = normalize(name);
    for &(key, factory) in MODEL_TAG_MAP {
        let normalized_key = normalize(key);
        if normalized.contains(&normalized_key) || normalized_key.contains(&normalized) {
            return factory();
        }
    }

    default_tags()
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

    #[test]
    fn test_with_overrides() {
        let tags = default_tags().with_overrides(
            Some("<custom_exec>"),
            None,
            Some(""),  // Empty should NOT override
            Some("<custom_output_close>"),
        );
        assert_eq!(tags.exec_open, "<custom_exec>");
        assert_eq!(tags.exec_close, "<SYSTEM.EXEC||>"); // None = keep default
        assert_eq!(tags.output_open, "<||SYSTEM.OUTPUT>"); // Empty = keep default
        assert_eq!(tags.output_close, "<custom_output_close>");
    }
}
