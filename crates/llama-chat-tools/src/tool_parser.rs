use regex::Regex;
use llama_chat_types::tool_tags::ToolTags;

// Default SYSTEM.EXEC regex (always tried as fallback)
// (?s) enables DOTALL mode so . matches newlines (multi-line commands)
// Closing tag: models may emit <SYSTEM.EXEC||> (correct) or <||SYSTEM.EXEC||> (mirrored opening)
lazy_static::lazy_static! {
    pub static ref EXEC_PATTERN: Regex = Regex::new(
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

    // Gemma 4 format: call:function_name{key:<|"|>value<|"|>,key2:value2}
    // Extracted from inside <|tool_call>...<tool_call|> tags.
    static ref GEMMA4_CALL_PATTERN: Regex = Regex::new(
        r"(?s)call:(\w+)\{(.*)\}"
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
pub fn extract_balanced_json(text: &str, start: usize) -> Option<(usize, String)> {
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
pub fn build_model_exec_regex(tags: &ToolTags) -> Option<Regex> {
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

pub type FormatDetector = fn(&str, &ToolTags) -> Option<String>;

/// Detector priority order. First match wins.
pub const FORMAT_PRIORITY: &[(&str, FormatDetector)] = &[
    ("model_specific", detect_model_specific),
    ("gemma4", detect_gemma4),
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

/// Detect Gemma 4 tool call format: call:function_name{key:<|"|>value<|"|>,key:value}
/// This format appears inside <|tool_call>...<tool_call|> tags OR as raw text.
/// Converts to standard JSON: {"name":"function_name","arguments":{...}}
fn detect_gemma4(text: &str, _tags: &ToolTags) -> Option<String> {
    let caps = GEMMA4_CALL_PATTERN.captures(text)?;
    let tool_name = caps.get(1)?.as_str();
    let args_raw = caps.get(2)?.as_str();

    // Parse Gemma 4 key-value format into JSON
    // Values can be: <|"|>string<|"|>, true, false, numbers, or nested structures
    let json_args = gemma4_args_to_json(args_raw);
    Some(format!(
        r#"{{"name":"{}","arguments":{}}}"#,
        tool_name, json_args
    ))
}

/// Convert Gemma 4 argument format to JSON string.
/// Input: `background:false,command:<|"|>mkdir foo<|"|>`
/// Output: `{"background":false,"command":"mkdir foo"}`
fn gemma4_args_to_json(raw: &str) -> String {
    let mut result = String::from("{");
    let mut first = true;
    let mut pos = 0;
    let bytes = raw.as_bytes();

    while pos < bytes.len() {
        // Skip whitespace
        while pos < bytes.len() && bytes[pos].is_ascii_whitespace() { pos += 1; }
        if pos >= bytes.len() { break; }

        // Parse key (until ':')
        let key_start = pos;
        while pos < bytes.len() && bytes[pos] != b':' { pos += 1; }
        if pos >= bytes.len() { break; }
        let key = raw[key_start..pos].trim();
        pos += 1; // skip ':'

        // Parse value
        while pos < bytes.len() && bytes[pos].is_ascii_whitespace() { pos += 1; }
        if pos >= bytes.len() { break; }

        let (value, new_pos) = if raw[pos..].starts_with("<|\"|>") {
            // String value delimited by <|"|>...<|"|>
            let content_start = pos + 5; // len("<|\"|>")
            if let Some(end) = raw[content_start..].find("<|\"|>") {
                let val = &raw[content_start..content_start + end];
                // Escape for JSON
                let escaped = val.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n");
                (format!("\"{}\"", escaped), content_start + end + 5)
            } else {
                // No closing quote, take rest as string
                let val = &raw[content_start..];
                let escaped = val.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n");
                (format!("\"{}\"", escaped), raw.len())
            }
        } else if raw[pos..].starts_with("true") {
            ("true".to_string(), pos + 4)
        } else if raw[pos..].starts_with("false") {
            ("false".to_string(), pos + 5)
        } else if bytes[pos] == b'{' {
            // Nested object — find balanced braces
            if let Some((end, nested)) = extract_balanced_json(raw, pos) {
                (nested, end)
            } else {
                break;
            }
        } else if bytes[pos] == b'[' {
            // Array — find balanced brackets
            let bracket_start = pos;
            let mut depth = 0;
            while pos < bytes.len() {
                if bytes[pos] == b'[' { depth += 1; }
                if bytes[pos] == b']' { depth -= 1; if depth == 0 { pos += 1; break; } }
                pos += 1;
            }
            (raw[bracket_start..pos].to_string(), pos)
        } else {
            // Number or other literal
            let val_start = pos;
            while pos < bytes.len() && bytes[pos] != b',' && !bytes[pos].is_ascii_whitespace() {
                pos += 1;
            }
            (raw[val_start..pos].to_string(), pos)
        };

        if !first { result.push(','); }
        first = false;
        result.push_str(&format!("\"{}\":{}", key, value));
        pos = new_pos;

        // Skip comma separator
        while pos < bytes.len() && (bytes[pos] == b',' || bytes[pos].is_ascii_whitespace()) {
            pos += 1;
        }
    }

    result.push('}');
    result
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
