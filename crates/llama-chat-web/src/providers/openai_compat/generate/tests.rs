//! Tests for the OpenAI-compatible provider.
//! Uses `super::super::*` to access `openai_compat` exports (get_preset, is_openai_compat, etc.)

#[cfg(test)]
mod tests {
    use super::super::super::*;
    // super::super::super = openai_compat module (via generate/mod.rs -> openai_compat/mod.rs)
    use crate::providers::openai_compat_request::{
        budget_trimmed_messages, execute_openai_tool, provider_cost_per_million,
        truncate_tool_output,
    };

    #[test]
    fn test_preset_lookup() {
        assert!(get_preset("groq").is_some());
        assert!(get_preset("gemini").is_some());
        assert!(get_preset("unknown_xyz").is_none());
    }

    #[test]
    fn test_is_openai_compat() {
        assert!(is_openai_compat("groq"));
        assert!(is_openai_compat("cerebras"));
        assert!(!is_openai_compat("claude_code"));
        assert!(!is_openai_compat("local"));
    }

    #[test]
    fn test_resolve_api_key_from_json() {
        let json = r#"{"groq": {"api_key": "gsk_test123"}, "gemini": "gem_key456"}"#;
        assert_eq!(resolve_api_key("groq", Some(json)), Some("gsk_test123".to_string()));
        assert_eq!(resolve_api_key("gemini", Some(json)), Some("gem_key456".to_string()));
        assert_eq!(resolve_api_key("cerebras", Some(json)), None);
    }

    #[test]
    fn test_resolve_base_url() {
        assert_eq!(
            resolve_base_url("groq", None),
            Some("https://api.groq.com/openai/v1".to_string())
        );

        // Custom override
        let json = r#"{"groq": {"base_url": "http://localhost:8080/v1"}}"#;
        assert_eq!(
            resolve_base_url("groq", Some(json)),
            Some("http://localhost:8080/v1".to_string())
        );
    }

    #[test]
    fn test_resolve_model() {
        use crate::providers::openai_compat_request::resolve_model;
        assert_eq!(resolve_model("groq", Some("my-model")), "my-model");
        assert_eq!(resolve_model("groq", None), "llama-3.3-70b-versatile");
        assert_eq!(resolve_model("unknown", None), "default");
    }

    #[test]
    fn test_get_agentic_tools() {
        use crate::providers::openai_compat_request::get_agentic_tools;
        let tools = get_agentic_tools(None);
        assert!(!tools.is_empty());
        // All returned tools must have a valid function name
        for tool in &tools {
            assert!(tool["function"]["name"].as_str().is_some());
        }
    }

    #[test]
    fn test_execute_openai_tool_read_file() {
        let dir = std::env::temp_dir();
        let path = dir.join("openai_compat_test_read.txt");
        std::fs::write(&path, "hello from test").unwrap();

        let args = serde_json::json!({"path": path.to_string_lossy()}).to_string();
        let result = execute_openai_tool("read_file", &args, None, None);
        assert!(result.contains("hello from test"));

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_execute_openai_tool_write_file() {
        let dir = std::env::temp_dir();
        let path = dir.join("openai_compat_test_write.txt");

        let args = serde_json::json!({"path": path.to_string_lossy(), "content": "written by test"}).to_string();
        let result = execute_openai_tool("write_file", &args, None, None);
        assert!(result.contains("Written"), "unexpected result: {result}");

        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "written by test");

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_execute_openai_tool_edit_file() {
        let dir = std::env::temp_dir();
        let path = dir.join("openai_compat_test_edit.txt");
        std::fs::write(&path, "foo bar baz").unwrap();

        let args =
            serde_json::json!({"path": path.to_string_lossy(), "old_string": "bar", "new_string": "qux"})
                .to_string();
        let result = execute_openai_tool("edit_file", &args, None, None);
        assert!(result.contains("Edited"), "unexpected result: {result}");

        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "foo qux baz");

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_execute_openai_tool_list_directory() {
        let args = serde_json::json!({"path": "."}).to_string();
        let result = execute_openai_tool("list_directory", &args, None, None);
        // Should return something (at least Cargo.toml or src/)
        assert!(!result.is_empty());
    }

    #[test]
    fn test_execute_openai_tool_unknown() {
        let result = execute_openai_tool("nonexistent_tool", "{}", None, None);
        assert!(result.contains("Unknown tool"));
    }

    #[test]
    fn test_truncate_tool_output_short() {
        let short = "hello world";
        assert_eq!(truncate_tool_output(short, 100), short);
    }

    #[test]
    fn test_truncate_tool_output_long() {
        let long = "a".repeat(10_000);
        let result = truncate_tool_output(&long, 1000);
        assert!(result.len() < 10_000);
        assert!(result.contains("chars truncated"));
    }

    #[test]
    fn test_provider_cost_per_million() {
        assert!(provider_cost_per_million("deepseek", "deepseek-chat").is_some());
        assert!(provider_cost_per_million("gemini", "gemini-2.0-flash").is_none());
        assert!(provider_cost_per_million("unknown_provider", "model").is_none());
    }

    #[test]
    fn test_budget_trimmed_messages_truncates_old_tool_results() {
        let mut messages: Vec<serde_json::Value> = vec![
            serde_json::json!({"role": "user", "content": "do something"}),
        ];
        // Add enough messages to trigger trimming (>10)
        for i in 0..10 {
            messages.push(serde_json::json!({"role": "assistant", "content": format!("step {i}")}));
            messages.push(serde_json::json!({"role": "tool", "tool_call_id": format!("id_{i}"), "content": "x".repeat(500)}));
        }
        let original_len = messages.len();
        let trimmed = budget_trimmed_messages(&messages);
        // Length should remain the same (we truncate content, not remove messages)
        assert_eq!(trimmed.len(), original_len);
        // Early tool messages (index 2, within the trim zone) should be summarized
        let early_tool = trimmed[2].get("content").unwrap().as_str().unwrap();
        assert!(early_tool.contains("Output:") || early_tool.contains("chars"));
    }
}
