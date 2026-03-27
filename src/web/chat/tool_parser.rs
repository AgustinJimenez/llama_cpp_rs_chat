use regex::Regex;
use super::tool_tags::ToolTags;

// Default SYSTEM.EXEC regex (always tried as fallback)
// (?s) enables DOTALL mode so . matches newlines (multi-line commands)
// Closing tag: models may emit <SYSTEM.EXEC||> (correct) or <||SYSTEM.EXEC||> (mirrored opening)
lazy_static::lazy_static! {
    pub(crate) static ref EXEC_PATTERN: Regex = Regex::new(
        r"(?s)SYSTEM\.EXEC>(.+?)<(?:\|{1,2})?SYSTEM\.EXEC\|{1,2}>"
    ).unwrap();

    // Llama3/Hermes XML format: <function=tool_name> ... </function>
    // Some models (Qwen3-Coder) output this without a <tool_call> wrapper.
    static ref LLAMA3_FUNC_PATTERN: Regex = Regex::new(
        r"(?s)(<function=[a-z_]+>.*?</function>)"
    ).unwrap();

    // Harmony format (gpt-oss-20b):
    //   Hardcoded path: to= tool_name code<|message|>{...}<|call|>
    //   Jinja path:     to=functions.tool_name <|constrain|>json<|message|>{...}<|call|>
    // Both end with <|message|>JSON<|call|>, differ in prefix and middle.
    static ref HARMONY_CALL_PATTERN: Regex = Regex::new(
        r"(?s)to=\s*(?:functions\.)?(\w+)[\s\S]*?<\|message\|>(.*?)<\|call\|>"
    ).unwrap();

    // Mistral v2 bracket format (Devstral-Small-2-2512):
    // [TOOL_CALLS]tool_name[ARGS]{"arg":"val"}
    // Only matches the prefix — JSON body is extracted via balanced-brace scanner
    // because non-greedy \{.*?\} fails on nested JSON (e.g. write_file with JSON content).
    static ref MISTRAL_BRACKET_PREFIX: Regex = Regex::new(
        r"\[TOOL_CALLS\](\w+)\[ARGS\]"
    ).unwrap();

    // Mistral JSON format (Magistral-Small-2509):
    // [TOOL_CALLS]{"name":"tool_name","arguments":{...}}
    // The [TOOL_CALLS] tag is followed directly by a JSON object (no name[ARGS] separator).
    static ref MISTRAL_JSON_PREFIX: Regex = Regex::new(
        r"\[TOOL_CALLS\]\s*\{"
    ).unwrap();
}

/// Extract balanced JSON starting at position `start` in `text`.
/// `text[start]` must be `{`. Respects string quoting so nested `{}`
/// inside JSON strings don't break the match.
/// Returns `(end_exclusive, json_slice)` on success.
pub(crate) fn extract_balanced_json(text: &str, start: usize) -> Option<(usize, String)> {
    let bytes = text.as_bytes();
    if start >= bytes.len() || bytes[start] != b'{' {
        return None;
    }
    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut prev_backslash = false;
    for (i, &b) in bytes[start..].iter().enumerate() {
        if in_string {
            if b == b'"' && !prev_backslash {
                in_string = false;
            }
            prev_backslash = b == b'\\' && !prev_backslash;
        } else {
            match b {
                b'"' => {
                    in_string = true;
                    prev_backslash = false;
                }
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        let end = start + i + 1;
                        return Some((end, text[start..end].to_string()));
                    }
                }
                _ => {}
            }
        }
    }
    None
}

/// Build a regex that matches the model-specific exec tags.
/// Returns None if the tags are already covered by the default EXEC_PATTERN.
pub(crate) fn build_model_exec_regex(tags: &ToolTags) -> Option<Regex> {
    // Skip if using default SYSTEM.EXEC tags (already handled by EXEC_PATTERN)
    if tags.exec_open.contains("SYSTEM.EXEC") {
        return None;
    }

    // Escape special regex characters in the tags
    let open = regex::escape(&tags.exec_open);
    let close = regex::escape(&tags.exec_close);

    // Build pattern: open_tag(.+?)close_tag
    // (?s) enables DOTALL mode so . matches newlines (multi-line commands like python -c)
    //
    // NOTE: GLM models open tool calls with <tool_call> but close with <|end_of_box|>
    // (a vision bounding box marker they repurpose as tool call terminator).
    // We accept <|end_of_box|> as an alternative close ONLY when open is <tool_call>,
    // since using <|begin_of_box|> as an alternative *open* tag caused false positives
    // (GLM uses <|begin_of_box|> for thinking boxes -> matched non-tool text).
    let close_alt = if tags.exec_open == "<tool_call>" {
        let ebox = regex::escape("<|end_of_box|>");
        format!("(?:{close}|{ebox})")
    } else {
        close.to_string()
    };
    let pattern = format!(r"(?s){open}(.+?){close_alt}");
    Regex::new(&pattern).ok()
}

// --- Format detectors: each returns the extracted command text or None ---

pub(crate) type FormatDetector = fn(&str, &ToolTags) -> Option<String>;

/// Detector priority order. First match wins.
pub(crate) const FORMAT_PRIORITY: &[(&str, FormatDetector)] = &[
    ("model_specific", detect_model_specific),
    ("exec", detect_exec),
    ("llama3", detect_llama3),
    ("harmony", detect_harmony),
    ("mistral_bracket", detect_mistral_bracket),
    ("mistral_json", detect_mistral_json),
];

fn detect_model_specific(text: &str, tags: &ToolTags) -> Option<String> {
    let re = build_model_exec_regex(tags)?;
    re.captures(text)?.get(1).map(|m| m.as_str().to_string())
}

fn detect_exec(text: &str, _tags: &ToolTags) -> Option<String> {
    EXEC_PATTERN.captures(text)?.get(1).map(|m| m.as_str().to_string())
}

fn detect_llama3(text: &str, _tags: &ToolTags) -> Option<String> {
    LLAMA3_FUNC_PATTERN.captures(text)?.get(1).map(|m| m.as_str().to_string())
}

fn detect_harmony(text: &str, _tags: &ToolTags) -> Option<String> {
    let caps = HARMONY_CALL_PATTERN.captures(text)?;
    let (tool_name, args_json) = (caps.get(1)?, caps.get(2)?);
    Some(format!(
        r#"{{"name":"{}","arguments":{}}}"#,
        tool_name.as_str(),
        args_json.as_str().trim()
    ))
}

fn detect_mistral_bracket(text: &str, _tags: &ToolTags) -> Option<String> {
    let caps = MISTRAL_BRACKET_PREFIX.captures(text)?;
    let tool_name = caps.get(1)?;
    let json_start = caps.get(0)?.end();
    let (_end, args_json) = extract_balanced_json(text, json_start)?;
    Some(format!(
        r#"{{"name":"{}","arguments":{}}}"#,
        tool_name.as_str(),
        args_json.trim()
    ))
}

fn detect_mistral_json(text: &str, _tags: &ToolTags) -> Option<String> {
    let m = MISTRAL_JSON_PREFIX.find(text)?;
    // The `{` is at the end of the match, so JSON starts at match.end() - 1
    let json_start = m.end() - 1;
    let (_end, json) = extract_balanced_json(text, json_start)?;
    // Validate it has the expected "name" and "arguments" fields
    let parsed: serde_json::Value = serde_json::from_str(&json).ok()?;
    if parsed.get("name").is_some() && parsed.get("arguments").is_some() {
        Some(json)
    } else {
        None
    }
}
