//! Per-model tool tag configuration.
//!
//! Different model families are trained with different tool-calling formats.
//! Using a model's native tags in the system prompt makes it far more likely
//! to follow tool-calling instructions than using generic custom tags.

// ── TagPair: dynamic tag pair system ──────────────────────────────────────────

/// A configurable tag pair representing any special token pair a model uses.
/// Stored as JSON in the database, editable by the user in the model config UI.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct TagPair {
    /// Category grouping: "tool", "thinking", "vision", "role", "control", "code_fim", "modifier"
    pub category: String,
    /// Identifier within category: "exec", "response", "think", "image", etc.
    pub name: String,
    /// Opening tag string (e.g., "<tool_call>", "<|user|>")
    pub open_tag: String,
    /// Closing tag string (empty for single-token markers like "<|user|>")
    pub close_tag: String,
    /// Whether this tag pair is active
    pub enabled: bool,
}

impl TagPair {
    fn new(category: &str, name: &str, open: &str, close: &str) -> Self {
        Self {
            category: category.to_string(),
            name: name.to_string(),
            open_tag: open.to_string(),
            close_tag: close.to_string(),
            enabled: true,
        }
    }

    fn pair(category: &str, name: &str, open: &str, close: &str) -> Self {
        Self::new(category, name, open, close)
    }

    fn single(category: &str, name: &str, open: &str) -> Self {
        Self::new(category, name, open, "")
    }
}

/// Tool tag delimiters used in the system prompt and command detection.
#[derive(Debug, Clone, serde::Serialize)]
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
/// GLM uses the same `<tool_call>`/`</tool_call>` format as Qwen (tokens 151352/151353).
/// Tool responses use `<tool_response>`/`</tool_response>` (tokens 151354/151355).
///
/// NOTE: `<|begin_of_box|>` / `<|end_of_box|>` (tokens 151361/151362) are VISION
/// bounding box markers for spatial grounding, NOT tool call tags. Previously these
/// were incorrectly used here, causing false positive tool call detections and
/// infinite generation loops.
///
/// `<|observation|>` (token 151338) is a critical stop token — it marks the boundary
/// where the model expects tool results. Must be configured as a stop token.
fn glm_tags() -> ToolTags {
    ToolTags::new(
        "<tool_call>",
        "</tool_call>",
        "<tool_response>",
        "</tool_response>",
    )
}

/// Tag factory function type for the model map.
type TagFactory = fn() -> ToolTags;

/// Known model name -> tag family mappings.
/// Keyed by `general.name` values from GGUF metadata.
/// SYNC: Must match MODEL_TOOL_TAGS in src/config/modelPresets.ts
const MODEL_TAG_MAP: &[(&str, TagFactory)] = &[
    // Qwen models - strong tool calling with native tags
    ("Qwen_Qwen3 Coder Next", qwen_tags),
    ("Qwen3 8B", qwen_tags),
    ("Qwen_Qwen3 30B A3B Instruct 2507", qwen_tags),
    ("Qwen3-Coder-30B-A3B-Instruct-1M", qwen_tags),
    ("Qwen3.5-35B-A3B", qwen_tags),
    ("Qwen_Qwen3.5 35B A3B", qwen_tags),
    // Mistral models - strong tool calling with native tags
    ("mistralai_Devstral Small 2507", mistral_tags),
    ("mistralai_Devstral Small 2 24B Instruct 2512", mistral_tags),
    ("Magistral-Small-2509", mistral_tags),
    ("mistralai_Ministral 3 3B Instruct 2512 BF16", mistral_tags),
    ("mistralai_Ministral 3 14B Reasoning 2512", mistral_tags),
    // Harmony models - native Harmony format tool calling
    ("Openai_Gpt Oss 20b", harmony_tags),
    // GLM models - native <tool_call> tags (same special tokens as Qwen)
    ("Zai org_GLM 4.6V Flash", glm_tags),
    ("Zai org_GLM 4.7 Flash", glm_tags),
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

// ── Tag pair preset factories ─────────────────────────────────────────────────

/// All known tag pairs for GLM-4 family models.
fn glm_tag_pairs() -> Vec<TagPair> {
    vec![
        // Tool calling
        TagPair::pair("tool", "exec", "<tool_call>", "</tool_call>"),
        TagPair::pair("tool", "response", "<tool_response>", "</tool_response>"),
        TagPair::pair("tool", "arg_key", "<arg_key>", "</arg_key>"),
        TagPair::pair("tool", "arg_value", "<arg_value>", "</arg_value>"),
        // Thinking
        TagPair::pair("thinking", "think", "<think>", "</think>"),
        // Vision / multimodal
        TagPair::pair("vision", "image", "<|begin_of_image|>", "<|end_of_image|>"),
        TagPair::pair("vision", "video", "<|begin_of_video|>", "<|end_of_video|>"),
        TagPair::pair("vision", "box", "<|begin_of_box|>", "<|end_of_box|>"),
        TagPair::pair("vision", "audio", "<|begin_of_audio|>", "<|end_of_audio|>"),
        TagPair::pair("vision", "transcription", "<|begin_of_transcription|>", "<|end_of_transcription|>"),
        // Role markers
        TagPair::single("role", "system", "<|system|>"),
        TagPair::single("role", "user", "<|user|>"),
        TagPair::single("role", "assistant", "<|assistant|>"),
        TagPair::single("role", "observation", "<|observation|>"),
        // Control
        TagPair::single("control", "eof", "<|endoftext|>"),
        TagPair::single("control", "sop", "<sop>"),
        // Modifier
        TagPair::single("modifier", "nothink", "/nothink"),
    ]
}

/// All known tag pairs for Qwen family models.
fn qwen_tag_pairs() -> Vec<TagPair> {
    vec![
        TagPair::pair("tool", "exec", "<tool_call>", "</tool_call>"),
        TagPair::pair("tool", "response", "<tool_response>", "</tool_response>"),
        TagPair::pair("thinking", "think", "<think>", "</think>"),
        TagPair::single("role", "system", "<|im_start|>system"),
        TagPair::single("role", "user", "<|im_start|>user"),
        TagPair::single("role", "assistant", "<|im_start|>assistant"),
        TagPair::single("control", "im_end", "<|im_end|>"),
        TagPair::single("control", "endoftext", "<|endoftext|>"),
    ]
}

/// All known tag pairs for Mistral family models.
fn mistral_tag_pairs() -> Vec<TagPair> {
    vec![
        TagPair::pair("tool", "exec", "[TOOL_CALLS]", "[/TOOL_CALLS]"),
        TagPair::pair("tool", "response", "[TOOL_RESULTS]", "[/TOOL_RESULTS]"),
        TagPair::single("control", "eos", "</s>"),
    ]
}

/// All known tag pairs for Harmony (gpt-oss-20b) family.
fn harmony_tag_pairs() -> Vec<TagPair> {
    vec![
        TagPair::single("tool", "call_prefix", "<|start|>tool"),
        TagPair::single("tool", "message", "<|message|>"),
        TagPair::single("tool", "call_end", "<|call|>"),
        TagPair::single("control", "start", "<|start|>"),
        TagPair::single("control", "end", "<|end|>"),
    ]
}

/// Default tag pairs for unknown models — just the 4 SYSTEM.EXEC tool tags.
fn default_tag_pairs() -> Vec<TagPair> {
    vec![
        TagPair::pair("tool", "exec", "<||SYSTEM.EXEC>", "<SYSTEM.EXEC||>"),
        TagPair::pair("tool", "response", "<||SYSTEM.OUTPUT>", "<SYSTEM.OUTPUT||>"),
    ]
}

/// Tag pair factory type.
type TagPairFactory = fn() -> Vec<TagPair>;

/// Known model name -> tag pair preset mappings.
/// SYNC: Must match MODEL_TAG_PAIRS in src/config/modelPresets.ts
const MODEL_TAG_PAIR_MAP: &[(&str, TagPairFactory)] = &[
    // Qwen models
    ("Qwen_Qwen3 Coder Next", qwen_tag_pairs),
    ("Qwen3 8B", qwen_tag_pairs),
    ("Qwen_Qwen3 30B A3B Instruct 2507", qwen_tag_pairs),
    ("Qwen3-Coder-30B-A3B-Instruct-1M", qwen_tag_pairs),
    ("Qwen3.5-35B-A3B", qwen_tag_pairs),
    ("Qwen_Qwen3.5 35B A3B", qwen_tag_pairs),
    // Mistral models
    ("mistralai_Devstral Small 2507", mistral_tag_pairs),
    ("mistralai_Devstral Small 2 24B Instruct 2512", mistral_tag_pairs),
    ("Magistral-Small-2509", mistral_tag_pairs),
    ("mistralai_Ministral 3 3B Instruct 2512 BF16", mistral_tag_pairs),
    ("mistralai_Ministral 3 14B Reasoning 2512", mistral_tag_pairs),
    // Harmony models
    ("Openai_Gpt Oss 20b", harmony_tag_pairs),
    // GLM models
    ("Zai org_GLM 4.6V Flash", glm_tag_pairs),
    ("Zai org_GLM 4.7 Flash", glm_tag_pairs),
];

/// Look up tag pairs for a model by its `general.name` from GGUF metadata.
/// Uses same fuzzy matching as `get_tool_tags_for_model`.
pub fn get_tag_pairs_for_model(general_name: Option<&str>) -> Vec<TagPair> {
    let name = match general_name {
        Some(n) if !n.is_empty() => n,
        _ => return default_tag_pairs(),
    };

    // Exact match
    for &(key, factory) in MODEL_TAG_PAIR_MAP {
        if key == name {
            return factory();
        }
    }

    // Fuzzy match
    let normalized = normalize(name);
    for &(key, factory) in MODEL_TAG_PAIR_MAP {
        let normalized_key = normalize(key);
        if normalized.contains(&normalized_key) || normalized_key.contains(&normalized) {
            return factory();
        }
    }

    default_tag_pairs()
}

/// Extract the 4 functional ToolTags from a tag pairs array.
/// Looks for category="tool" with name="exec" and name="response".
pub fn derive_tool_tags_from_pairs(pairs: &[TagPair]) -> Option<ToolTags> {
    let exec = pairs.iter().find(|p| p.category == "tool" && p.name == "exec" && p.enabled)?;
    let resp = pairs.iter().find(|p| p.category == "tool" && p.name == "response" && p.enabled)?;
    Some(ToolTags::new(&exec.open_tag, &exec.close_tag, &resp.open_tag, &resp.close_tag))
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

    // ── TagPair tests ──

    #[test]
    fn test_glm_tag_pairs_count() {
        let pairs = glm_tag_pairs();
        assert_eq!(pairs.len(), 17);
        assert!(pairs.iter().any(|p| p.category == "tool" && p.name == "exec"));
        assert!(pairs.iter().any(|p| p.category == "vision" && p.name == "image"));
        assert!(pairs.iter().any(|p| p.category == "role" && p.name == "observation"));
    }

    #[test]
    fn test_get_tag_pairs_glm() {
        let pairs = get_tag_pairs_for_model(Some("Zai org_GLM 4.6V Flash"));
        assert_eq!(pairs.len(), 17);
        assert_eq!(pairs[0].open_tag, "<tool_call>");
    }

    #[test]
    fn test_get_tag_pairs_unknown() {
        let pairs = get_tag_pairs_for_model(Some("SomeUnknownModel"));
        assert_eq!(pairs.len(), 2); // default: exec + response
        assert_eq!(pairs[0].open_tag, "<||SYSTEM.EXEC>");
    }

    #[test]
    fn test_derive_tool_tags_from_pairs() {
        let pairs = glm_tag_pairs();
        let tags = derive_tool_tags_from_pairs(&pairs).unwrap();
        assert_eq!(tags.exec_open, "<tool_call>");
        assert_eq!(tags.exec_close, "</tool_call>");
        assert_eq!(tags.output_open, "<tool_response>");
        assert_eq!(tags.output_close, "</tool_response>");
    }

    #[test]
    fn test_derive_tool_tags_missing_exec() {
        let pairs = vec![
            TagPair::pair("tool", "response", "<tool_response>", "</tool_response>"),
        ];
        assert!(derive_tool_tags_from_pairs(&pairs).is_none());
    }

    #[test]
    fn test_tag_pair_serialization() {
        let pair = TagPair::pair("tool", "exec", "<tool_call>", "</tool_call>");
        let json = serde_json::to_string(&pair).unwrap();
        let deserialized: TagPair = serde_json::from_str(&json).unwrap();
        assert_eq!(pair, deserialized);
    }
}
