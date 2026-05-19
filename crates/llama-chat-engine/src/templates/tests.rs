use super::*;

#[test]
fn test_universal_system_prompt_contains_exec_tags() {
    use crate::tool_tags;
    let prompt = get_universal_system_prompt_with_tags(&tool_tags::default_tags());
    assert!(prompt.contains("<||SYSTEM.EXEC>"));
    assert!(prompt.contains("<SYSTEM.EXEC||>"));
    assert!(prompt.contains("<||SYSTEM.OUTPUT>"));
    assert!(prompt.contains("<SYSTEM.OUTPUT||>"));
}

#[test]
fn test_universal_system_prompt_contains_os_info() {
    use crate::tool_tags;
    let prompt = get_universal_system_prompt_with_tags(&tool_tags::default_tags());
    assert!(prompt.contains("OS:"));
    assert!(prompt.contains("Working Directory:"));
    assert!(prompt.contains("Shell:"));
}

#[test]
fn test_template_preserves_multiline_content() {
    let conversation = "USER:\nLine 1\nLine 2\nLine 3";
    let result = apply_model_chat_template(conversation, Some("ChatML")).unwrap();

    assert!(result.contains("Line 1"));
    assert!(result.contains("Line 2"));
    assert!(result.contains("Line 3"));
}

#[test]
fn test_template_handles_empty_content() {
    let conversation = "USER:\n\n\nASSISTANT:\n";
    let result = apply_model_chat_template(conversation, Some("ChatML"));
    assert!(result.is_ok());
}

#[test]
fn test_template_includes_universal_prompt() {
    let conversation = "USER:\nTest message";
    let result = apply_model_chat_template(conversation, Some("ChatML")).unwrap();
    assert!(result.contains("<||SYSTEM.EXEC>"));
}

#[test]
fn test_all_templates_include_system_exec() {
    let conversation = "USER:\nTest message";

    for template in &["ChatML", "Mistral", "Llama3", "Gemma"] {
        let result = apply_model_chat_template(conversation, Some(template)).unwrap();
        assert!(
            result.contains("<||SYSTEM.EXEC>"),
            "Template {template} should include SYSTEM.EXEC"
        );
    }
}

#[test]
fn test_model_specific_tags_in_prompt() {
    use crate::tool_tags;

    let qwen_tags = tool_tags::get_tool_tags_for_model(Some("Qwen3 8B"));
    let prompt = get_universal_system_prompt_with_tags(&qwen_tags);
    assert!(prompt.contains("<tool_call>"), "Qwen prompt should use <tool_call> tags");
    assert!(prompt.contains("</tool_call>"), "Qwen prompt should use </tool_call> tags");
    assert!(!prompt.contains("SYSTEM.EXEC"), "Qwen prompt should NOT contain SYSTEM.EXEC");

    let mistral_tags = tool_tags::get_tool_tags_for_model(Some("mistralai_Devstral Small 2507"));
    let prompt = get_universal_system_prompt_with_tags(&mistral_tags);
    assert!(prompt.contains("[TOOL_CALLS]"), "Mistral prompt should use [TOOL_CALLS] tags");
    assert!(prompt.contains("[/TOOL_CALLS]"), "Mistral prompt should use [/TOOL_CALLS] tags");

    let default_tags = tool_tags::get_tool_tags_for_model(Some("SomeUnknownModel"));
    let prompt = get_universal_system_prompt_with_tags(&default_tags);
    assert!(prompt.contains("<||SYSTEM.EXEC>"), "Unknown model should use default SYSTEM.EXEC tags");
}
