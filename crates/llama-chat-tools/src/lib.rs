//! Native tool implementations for LLM agent tool calls.
//!
//! Provides safe, shell-free implementations of common operations that LLM agents
//! need: reading/writing files, running Python code, listing directories, browser
//! interaction, MCP server management, and tool call parsing for multiple formats.

pub use llama_chat_types::NativeToolResult;

use serde_json::Value;

pub mod file_tools;
pub mod search_tools;
pub mod command_tools;
pub mod parsing;
pub mod doc_extractors;
pub mod browser_tools;
pub mod browser_session;
#[cfg(feature = "wry-browser")]
pub mod wry_browser;
pub mod mcp_tools;
pub mod screenshot_tool;
pub mod telegram;
pub mod tool_parser;
pub mod tool_defs;
mod dispatch;
mod utils;

pub use dispatch::{
    dispatch_native_tool, extract_execute_command_with_opts, extract_tool_args_summary,
    extract_tool_name,
};
pub use doc_extractors::*;
pub use file_tools::{read_with_encoding_detection, truncate_text_content};
pub use parsing::*;
#[allow(unused_imports)]
pub use screenshot_tool::tool_take_screenshot_with_image;
pub use tool_defs::all_tool_definitions;
pub use tool_parser::{
    build_model_exec_regex, extract_balanced_json, FormatDetector, EXEC_PATTERN,
    FORMAT_PRIORITY,
};

/// Trait for MCP manager operations needed by the tools crate.
/// The root crate implements this for its concrete `McpManager` type.
pub trait McpManagerOps: Send + Sync {
    fn is_mcp_tool(&self, name: &str) -> bool;
    fn call_tool(&self, qualified_name: &str, args: Value) -> Result<String, String>;
    fn get_server_statuses(&self) -> Vec<llama_chat_types::McpServerStatus>;
    fn get_tool_definitions(&self) -> Vec<McpToolDefInfo>;
    fn get_connected_server_names(&self) -> Vec<String>;
    fn refresh_connections(&self, db: &llama_chat_db::SharedDatabase) -> Result<(), String>;
}

/// Minimal tool definition info for MCP tools.
#[derive(Debug, Clone)]
pub struct McpToolDefInfo {
    pub qualified_name: String,
    pub description: String,
    pub input_schema: Value,
    pub server_name: String,
}

impl McpToolDefInfo {
    pub fn to_openai_function(&self) -> Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": self.qualified_name,
                "description": format!("[MCP:{}] {}", self.server_name, self.description),
                "parameters": self.input_schema,
            }
        })
    }
}

/// External functions that the tools crate needs from the root crate.
pub struct DispatchContext<'a> {
    pub get_tool_catalog: Option<&'a dyn Fn(&str) -> String>,
    pub get_tool_schema: Option<&'a dyn Fn(&str) -> Option<String>>,
    pub discover_skills: Option<&'a dyn Fn(&std::path::Path) -> Vec<SkillInfo>>,
    pub get_skill: Option<&'a dyn Fn(&std::path::Path, &str) -> Option<SkillInfo>>,
}

/// Minimal skill info for the tools crate.
#[derive(Debug, Clone)]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
    pub content: String,
}
