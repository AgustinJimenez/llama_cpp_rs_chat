//! Tool tag type definitions.
//!
//! The detection functions (get_tool_tags_for_model, get_tag_pairs_for_model, TAG_PAIR_DB)
//! remain in the main crate at src/web/chat/tool_tags.rs.

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
    pub fn new(category: &str, name: &str, open: &str, close: &str) -> Self {
        Self {
            category: category.to_string(),
            name: name.to_string(),
            open_tag: open.to_string(),
            close_tag: close.to_string(),
            enabled: true,
        }
    }

    pub fn pair(category: &str, name: &str, open: &str, close: &str) -> Self {
        Self::new(category, name, open, close)
    }

    pub fn single(category: &str, name: &str, open: &str) -> Self {
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
    pub fn new(exec_open: &str, exec_close: &str, output_open: &str, output_close: &str) -> Self {
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
