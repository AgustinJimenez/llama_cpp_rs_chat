//! Compound desktop tools that combine multiple primitives into single actions.

mod text_search;
mod element_ops;
mod dialog_nav;
mod smart_actions;

// Re-export all public tool functions for backward compatibility
pub use text_search::{tool_find_and_click_text, tool_wait_for_text_on_screen};
pub use element_ops::{tool_type_into_element, tool_get_window_text, tool_drag_and_drop_element, tool_scroll_element};
pub use dialog_nav::{tool_file_dialog_navigate, tool_get_context_menu};
pub use smart_actions::{tool_smart_wait, tool_click_and_verify};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_and_click_text_missing_text() {
        let args = serde_json::json!({});
        let result = tool_find_and_click_text(&args);
        assert!(result.text.contains("Error [find_and_click_text]"));
        assert!(result.text.contains("'text' is required"));
    }

    #[test]
    fn test_type_into_element_missing_text() {
        let args = serde_json::json!({});
        let result = tool_type_into_element(&args);
        assert!(result.text.contains("Error [type_into_element]"));
        assert!(result.text.contains("'text' is required"));
    }

    #[test]
    fn test_type_into_element_missing_name_and_type() {
        let args = serde_json::json!({"text": "hello"});
        let result = tool_type_into_element(&args);
        assert!(result.text.contains("Error [type_into_element]"));
        assert!(result.text.contains("'name' or 'control_type'"));
    }

    #[test]
    fn test_drag_and_drop_element_missing_from() {
        let args = serde_json::json!({"to_name": "target"});
        let result = tool_drag_and_drop_element(&args);
        assert!(result.text.contains("Error [drag_and_drop_element]"));
        assert!(result.text.contains("from_name"));
    }

    #[test]
    fn test_scroll_element_missing_name_and_type() {
        let args = serde_json::json!({"direction": "down"});
        let result = tool_scroll_element(&args);
        assert!(result.text.contains("Error [scroll_element]"));
        assert!(result.text.contains("'name' or 'control_type'"));
    }

    #[test]
    fn test_get_context_menu_missing_x() {
        let args = serde_json::json!({"y": 100});
        let result = tool_get_context_menu(&args);
        assert!(result.text.contains("Error [get_context_menu]"));
        assert!(result.text.contains("'x'"));
    }
}
