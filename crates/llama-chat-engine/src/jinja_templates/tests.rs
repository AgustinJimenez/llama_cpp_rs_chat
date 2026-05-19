#[cfg(test)]
mod tests {
    use super::super::{
        apply_native_chat_template, parse_conversation_for_jinja,
        get_available_tools_openai,
        ChatMessage,
    };
    use super::super::tool_catalog::*;
    // preprocess_template and epoch_days_to_ymd are pub(crate), access via parent
    use crate::jinja_templates::{preprocess_template, epoch_days_to_ymd};

    #[test]
    fn test_parse_conversation_for_jinja_replaces_system() {
        let conversation = r#"SYSTEM:
Old system prompt that should be replaced.

USER:
Hello!

ASSISTANT:
Hi there!"#;

        let messages = parse_conversation_for_jinja(conversation, "My behavioral prompt");
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].role, "system");
        assert_eq!(messages[0].content, "My behavioral prompt");
        assert_eq!(messages[1].role, "user");
        assert_eq!(messages[1].content, "Hello!");
        assert_eq!(messages[2].role, "assistant");
        assert_eq!(messages[2].content, "Hi there!");
    }

    #[test]
    fn test_get_available_tools_openai_format() {
        let tools = get_available_tools_openai();
        assert!(!tools.is_empty());
        for tool in &tools {
            assert_eq!(tool["type"], "function");
            assert!(tool["function"]["name"].is_string());
            assert!(tool["function"]["description"].is_string());
        }
    }

    #[test]
    fn test_preprocess_template_strips_ensure_ascii() {
        let input = r#"{{ tool | tojson(ensure_ascii=False) }}"#;
        let output = preprocess_template(input);
        assert_eq!(output, "{{ tool | tojson }}");
    }

    #[test]
    fn test_preprocess_template_converts_endswith() {
        let input = r#"not visible_text(m.content).endswith("/nothink")"#;
        let output = preprocess_template(input);
        assert_eq!(output, r#"not visible_text(m.content) is endingwith("/nothink")"#);
    }

    #[test]
    fn test_preprocess_template_converts_startswith() {
        let input = r#"message.content.startswith('<tool_response>')"#;
        let output = preprocess_template(input);
        assert_eq!(output, r#"message.content is startingwith('<tool_response>')"#);
    }

    #[test]
    fn test_preprocess_template_converts_strip() {
        let input = r#"{{ content.strip() }}"#;
        let output = preprocess_template(input);
        assert_eq!(output, r#"{{ content | trim }}"#);
    }

    #[test]
    fn test_simple_chatml_jinja_render() {
        let template = r#"{%- for message in messages %}
<|im_start|>{{ message.role }}
{{ message.content }}<|im_end|>
{%- endfor %}
{%- if add_generation_prompt %}
<|im_start|>assistant
{%- endif %}"#;

        let messages = vec![
            ChatMessage {
                role: "system".to_string(),
                content: "You are helpful.".to_string(),
                tool_calls: None,
            },
            ChatMessage {
                role: "user".to_string(),
                content: "Hello!".to_string(),
                tool_calls: None,
            },
        ];

        let result = apply_native_chat_template(
            template,
            messages,
            None,
            None,
            true,
            "<s>",
            "</s>",
            false,
        );
        assert!(result.is_ok());
        let prompt = result.unwrap();
        assert!(prompt.contains("<|im_start|>system"));
        assert!(prompt.contains("You are helpful."));
        assert!(prompt.contains("<|im_start|>user"));
        assert!(prompt.contains("Hello!"));
        assert!(prompt.contains("<|im_start|>assistant"));
    }

    #[test]
    fn test_raise_exception_works() {
        let template = r#"{% if true %}{{ raise_exception("test error") }}{% endif %}"#;
        let messages = vec![];
        let result = apply_native_chat_template(template, messages, None, None, false, "", "", false);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("test error"));
    }

    #[test]
    fn test_strftime_now_works() {
        let template = r#"{{ strftime_now("%Y-%m-%d") }}"#;
        let messages = vec![];
        let result = apply_native_chat_template(template, messages, None, None, false, "", "", false);
        assert!(result.is_ok());
        let date = result.unwrap();
        assert_eq!(date.len(), 10);
        assert_eq!(&date[4..5], "-");
        assert_eq!(&date[7..8], "-");
    }

    #[test]
    fn test_epoch_days_to_ymd() {
        let (y, m, d) = epoch_days_to_ymd(20513);
        assert_eq!((y, m, d), (2026, 3, 1));
        let (y, m, d) = epoch_days_to_ymd(0);
        assert_eq!((y, m, d), (1970, 1, 1));
    }
}
