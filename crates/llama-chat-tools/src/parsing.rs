//! JSON parsing and fixup functions for LLM-generated tool calls.
//!
//! Handles the many quirky JSON formats that different models produce:
//! standard JSON, Mistral comma-delimited, Llama3 XML, GLM XML, LFM2 Python calls,
//! and various broken JSON patterns (unescaped newlines, invalid backslashes, missing braces).

use serde_json::Value;

#[path = "parsing/formats.rs"]
mod formats;
use formats::{
    try_infer_tool_from_bare_args, try_parse_all_json_calls, try_parse_glm_xml_format,
    try_parse_json_format, try_parse_lfm2_python_call_format, try_parse_llama3_xml_format,
    try_parse_mistral_comma_format, try_parse_name_json_format,
};

/// Escape raw newlines inside JSON string values so serde_json can parse them.
/// Models often emit multiline content like `"content": "line1\nline2"` with literal
/// newlines instead of `\\n`, which is invalid JSON.
pub fn escape_newlines_in_json_strings(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut in_string = false;
    let mut prev_was_backslash = false;
    for ch in input.chars() {
        if in_string {
            if ch == '\n' {
                result.push_str("\\n");
                prev_was_backslash = false;
                continue;
            }
            if ch == '\r' {
                prev_was_backslash = false;
                continue; // drop \r, \n will follow
            }
            if ch == '"' && !prev_was_backslash {
                in_string = false;
            }
            prev_was_backslash = ch == '\\' && !prev_was_backslash;
        } else if ch == '"' {
            in_string = true;
            prev_was_backslash = false;
        }
        result.push(ch);
    }
    result
}

/// Auto-close unbalanced JSON braces/brackets (ignoring those inside strings).
/// Models sometimes omit the final `}` or `}}` when generating tool-call JSON.
pub fn auto_close_json(input: &str) -> String {
    let mut depth_brace: i32 = 0; // { }
    let mut depth_bracket: i32 = 0; // [ ]
    let mut in_string = false;
    let mut prev_backslash = false;

    for ch in input.chars() {
        if in_string {
            if ch == '"' && !prev_backslash {
                in_string = false;
            }
            prev_backslash = ch == '\\' && !prev_backslash;
        } else {
            match ch {
                '"' => {
                    in_string = true;
                    prev_backslash = false;
                }
                '{' => depth_brace += 1,
                '}' => depth_brace -= 1,
                '[' => depth_bracket += 1,
                ']' => depth_bracket -= 1,
                _ => {}
            }
        }
    }

    let mut result = input.to_string();
    for _ in 0..depth_bracket {
        result.push(']');
    }
    for _ in 0..depth_brace {
        result.push('}');
    }
    result
}

/// Try to parse JSON, applying progressive fixups on failure:
/// Escape invalid backslash sequences inside JSON strings.
/// Models generating PHP/C# code produce `\D`, `\M`, `\E` etc. from namespace paths
/// like `Illuminate\Database\Eloquent\Model`. These are invalid JSON escapes that
/// cause serde_json to reject the entire tool call. This function converts them to
/// valid `\\D`, `\\M`, `\\E` (literal backslash + letter) while preserving valid
/// JSON escapes (`\"`, `\\`, `\/`, `\b`, `\f`, `\n`, `\r`, `\t`, `\uXXXX`).
pub fn escape_invalid_backslashes_in_strings(input: &str) -> String {
    let mut result = String::with_capacity(input.len() + input.len() / 8);
    let mut in_string = false;
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if in_string {
            if ch == '\\' {
                if let Some(&next) = chars.peek() {
                    match next {
                        // Valid JSON escape sequences — keep as-is and consume the next char
                        '"' | '\\' | '/' | 'b' | 'f' | 'n' | 'r' | 't' | 'u' => {
                            result.push(ch);
                            result.push(chars.next().unwrap());
                        }
                        // Invalid escape — double the backslash so serde_json sees \\X
                        _ => {
                            result.push('\\');
                            result.push(ch);
                        }
                    }
                } else {
                    // Trailing backslash at end of input — escape it
                    result.push('\\');
                    result.push(ch);
                }
            } else if ch == '"' {
                in_string = false;
                result.push(ch);
            } else {
                result.push(ch);
            }
        } else {
            if ch == '"' {
                in_string = true;
            }
            result.push(ch);
        }
    }

    result
}

/// Escape unescaped inner quotes inside JSON strings.
///
/// LLMs generating write_file with JSON content often produce:
///   {"path": "test.json", "content": "{\n  "key": "val"\n}"}
/// where the inner `"key"` quotes are NOT escaped. This function detects
/// quotes that appear to be INSIDE a string (not structural) by checking
/// the character following the `"`. Structural quotes are followed by
/// `,`, `:`, `}`, `]`, or whitespace-then-structural. Inner quotes are
/// followed by letters/digits (field names or values).
fn escape_inner_quotes_in_strings(input: &str) -> String {
    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut result = String::with_capacity(len + len / 8);
    let mut in_string = false;
    let mut prev_backslash = false;
    let mut i = 0;

    while i < len {
        let b = bytes[i];
        if in_string {
            if b == b'"' && !prev_backslash {
                // Check what follows this quote to decide if it's structural or inner
                let mut j = i + 1;
                while j < len && (bytes[j] == b' ' || bytes[j] == b'\t') {
                    j += 1;
                }
                let next = if j < len { bytes[j] } else { 0 };
                // Structural end-of-string: followed by , : } ] or EOF
                if next == b',' || next == b':' || next == b'}' || next == b']' || next == 0 {
                    in_string = false;
                    result.push('"');
                } else {
                    // Inner quote — escape it
                    result.push('\\');
                    result.push('"');
                }
            } else if b == b'\n' {
                result.push_str("\\n");
                prev_backslash = false;
                i += 1;
                continue;
            } else if b == b'\r' {
                i += 1;
                prev_backslash = false;
                continue;
            } else {
                prev_backslash = b == b'\\' && !prev_backslash;
                result.push(b as char);
            }
        } else {
            if b == b'"' {
                in_string = true;
                prev_backslash = false;
            }
            result.push(b as char);
        }
        i += 1;
    }
    result
}

/// Fix the broken format some models emit where the arguments object is wrapped
/// in a bare nested object instead of being a flat key:
///   `{"name": "X", {"arguments": {...}}}` → `{"name": "X", "arguments": {...}}`
///   `{"name": "X", {"key": "val"}}` → `{"name": "X", "arguments": {"key": "val"}}`
fn fix_bare_nested_object(input: &str) -> Option<String> {
    if !input.contains("\"name\"") {
        return None;
    }

    // Case 1: {"name": "X", {"arguments": VALUE}} — nested "arguments" wrapper
    // Replace `, {"arguments":` with `, "arguments":` and strip one trailing `}`
    if let Some(pos) = input.find(", {\"arguments\"") {
        let before = &input[..pos];
        let rest = &input[pos + ", {\"arguments\"".len()..];
        let candidate = format!("{before}, \"arguments\"{rest}");
        // Strip one trailing `}` (the extra one from the now-removed wrapping `{`)
        let trimmed = candidate.trim_end();
        if trimmed.ends_with('}') {
            let last = trimmed.rfind('}').unwrap();
            let fixed = trimmed[..last].trim_end().to_string();
            return Some(fixed);
        }
        return Some(candidate);
    }

    // Case 2: {"name": "X", {"key": "val"}} — bare args object with no "arguments" key
    // Find `, {"` that comes after the name field
    if let Some(name_end) = input.find("\", ") {
        let after_name = &input[name_end + 3..];
        if after_name.starts_with('{') {
            let before = &input[..name_end + 3];
            return Some(format!("{before}\"arguments\": {after_name}"));
        }
    }

    None
}

/// 1. Raw parse
/// 2. Escape literal newlines inside strings
/// 3. Escape invalid backslashes + newlines
/// 4. Auto-close missing braces/brackets
/// 5. Escape unescaped inner quotes (LLM JSON-in-JSON)
/// 6. Fix bare nested object (broken `{"name":"X", {...}}` format)
pub fn try_parse_with_fixups(input: &str) -> Option<Value> {
    // 1. Try as-is
    if let Ok(v) = serde_json::from_str::<Value>(input) {
        return Some(v);
    }
    // 2. Escape newlines
    let escaped_nl = escape_newlines_in_json_strings(input);
    if let Ok(v) = serde_json::from_str::<Value>(&escaped_nl) {
        return Some(v);
    }
    // 3. Escape invalid backslashes + newlines
    let escaped_bs = escape_invalid_backslashes_in_strings(input);
    let escaped_both = escape_newlines_in_json_strings(&escaped_bs);
    if let Ok(v) = serde_json::from_str::<Value>(&escaped_both) {
        return Some(v);
    }
    // 4. Escape + auto-close braces
    let closed = auto_close_json(&escaped_both);
    if let Ok(v) = serde_json::from_str::<Value>(&closed) {
        return Some(v);
    }
    // 5. Escape inner quotes (handles JSON-inside-JSON from LLMs)
    let escaped_quotes = escape_inner_quotes_in_strings(input);
    if let Ok(v) = serde_json::from_str::<Value>(&escaped_quotes) {
        return Some(v);
    }
    // 6. All fixups combined
    let all_fixed = escape_inner_quotes_in_strings(&escaped_both);
    let all_closed = auto_close_json(&all_fixed);
    if let Ok(v) = serde_json::from_str::<Value>(&all_closed) {
        return Some(v);
    }
    // 7. Fix bare nested object format: {"name":"X", {"arguments":{...}}} or {"name":"X", {...}}
    if let Some(fixed) = fix_bare_nested_object(input) {
        let fixed_bs = escape_invalid_backslashes_in_strings(&fixed);
        let fixed_nl = escape_newlines_in_json_strings(&fixed_bs);
        let fixed_closed = auto_close_json(&fixed_nl);
        if let Ok(v) = serde_json::from_str::<Value>(&fixed_closed) {
            return Some(v);
        }
    }
    None
}

/// Try to parse a tool call text into (name, arguments) using all supported formats.
///
/// Returns `Some((name, args))` if parsed, `None` otherwise.
pub fn try_parse_tool_call(text: &str) -> Option<(String, Value)> {
    let trimmed = text.trim();
    if let Some(result) = try_parse_json_format(trimmed) {
        Some(result)
    } else if let Some(result) = try_parse_lfm2_python_call_format(trimmed) {
        Some(result)
    } else if let Some(result) = try_parse_mistral_comma_format(trimmed) {
        Some(result)
    } else if let Some(result) = try_parse_llama3_xml_format(trimmed) {
        Some(result)
    } else if let Some(result) = try_parse_glm_xml_format(trimmed) {
        Some(result)
    } else {
        try_parse_name_json_format(trimmed)
    }
}

/// Parse ALL tool calls from raw text, supporting batch JSON arrays.
///
/// Tries JSON array/object first (can return multiple calls), then falls back
/// to single-call formats (Mistral comma, Llama3 XML, Name+JSON, bare args).
/// Returns empty vec if nothing could be parsed.
pub fn try_parse_all_from_raw(text: &str) -> Vec<(String, Value)> {
    let trimmed = text.trim();

    // JSON array/object — may contain multiple calls
    if let Some(calls) = try_parse_all_json_calls(trimmed) {
        return calls;
    }

    // Single-call formats — return as vec of 1
    if let Some(result) = try_parse_lfm2_python_call_format(trimmed) {
        return vec![result];
    }
    if let Some(result) = try_parse_mistral_comma_format(trimmed) {
        return vec![result];
    }
    if let Some(result) = try_parse_llama3_xml_format(trimmed) {
        return vec![result];
    }
    if let Some(result) = try_parse_glm_xml_format(trimmed) {
        return vec![result];
    }
    if let Some(result) = try_parse_name_json_format(trimmed) {
        return vec![result];
    }
    if let Some(result) = try_infer_tool_from_bare_args(trimmed) {
        return vec![result];
    }

    Vec::new()
}

/// Extract a boolean from a JSON value that may be a real bool or a string like "True"/"true"/"false".
/// Models using XML-based tool formats (Llama3 XML, GLM) emit parameter values as strings,
/// so "True" needs to be recognized as `true`.
pub fn value_as_bool_flexible(v: &Value) -> Option<bool> {
    if let Some(b) = v.as_bool() {
        return Some(b);
    }
    if let Some(s) = v.as_str() {
        match s.trim().to_lowercase().as_str() {
            "true" | "1" | "yes" => return Some(true),
            "false" | "0" | "no" => return Some(false),
            _ => {}
        }
    }
    None
}
