//! JSON parsing and fixup functions for LLM-generated tool calls.
//!
//! Handles the many quirky JSON formats that different models produce:
//! standard JSON, Mistral comma-delimited, Llama3 XML, GLM XML, LFM2 Python calls,
//! and various broken JSON patterns (unescaped newlines, invalid backslashes, missing braces).

use serde_json::Value;

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

/// 1. Raw parse
/// 2. Escape literal newlines inside strings
/// 3. Escape invalid backslashes + newlines
/// 4. Auto-close missing braces/brackets
/// 5. Escape unescaped inner quotes (LLM JSON-in-JSON)
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
    serde_json::from_str::<Value>(&all_closed).ok()
}

/// Extract (name, arguments) from a single JSON object that has "name" and optional "arguments".
fn extract_name_args(obj: &Value) -> Option<(String, Value)> {
    let name = obj.get("name")?.as_str()?.to_string();
    let args = obj
        .get("arguments")
        .cloned()
        .unwrap_or(Value::Object(serde_json::Map::new()));
    Some((name, args))
}

/// Parse standard JSON format: `{"name":"...","arguments":{...}}` or array `[{...}]`
/// Returns only the first tool call (backward compat).
fn try_parse_json_format(trimmed: &str) -> Option<(String, Value)> {
    try_parse_all_json_calls(trimmed)
        .and_then(|v| v.into_iter().next())
}

/// Parse ALL tool calls from JSON: single object or array of objects.
/// Returns `Vec<(name, arguments)>` with one or more entries.
fn try_parse_all_json_calls(trimmed: &str) -> Option<Vec<(String, Value)>> {
    let parsed = try_parse_with_fixups(trimmed)?;

    if let Some(arr) = parsed.as_array() {
        let calls: Vec<(String, Value)> = arr
            .iter()
            .filter_map(|item| extract_name_args(item))
            .collect();
        if calls.is_empty() {
            None
        } else {
            Some(calls)
        }
    } else {
        // Single object
        let call = extract_name_args(&parsed)?;
        Some(vec![call])
    }
}

/// Parse Mistral comma-delimited format: `tool_name,{"arg":"val"}`
///
/// Devstral outputs `[TOOL_CALLS]read_file,{"path":"file.txt"}[/TOOL_CALLS]`
/// where the function name and JSON args are separated by a comma.
fn try_parse_mistral_comma_format(trimmed: &str) -> Option<(String, Value)> {
    // Find the first comma followed by `{` — that's the split point
    let comma_idx = trimmed.find(",{")?;
    let name = trimmed[..comma_idx].trim().to_string();
    let json_part = &trimmed[comma_idx + 1..]; // skip the comma

    // The JSON part is the arguments directly (not wrapped in "arguments")
    let args: Value = serde_json::from_str(json_part).ok()?;

    // Validate: name should be a simple identifier (no spaces, no special chars)
    if name.is_empty() || name.contains(' ') || name.contains('{') {
        return None;
    }

    // Wrap into standard format: the JSON IS the arguments
    Some((name, args))
}

/// Parse direct concatenation format: `tool_name{"arg":"val"}`
///
/// Granite models output tool calls with the name directly followed by JSON args,
/// e.g. `list_directory{"path": "."}` or `read_file{"path": "test.txt"}`.
fn try_parse_name_json_format(trimmed: &str) -> Option<(String, Value)> {
    // Find the first `{` — everything before it is the name
    let brace_idx = trimmed.find('{')?;
    let name = trimmed[..brace_idx].trim().to_string();
    let json_part = &trimmed[brace_idx..];

    // Validate: name should be a simple identifier
    if name.is_empty() || name.contains(' ') || name.contains('<') || name.contains('>') {
        return None;
    }

    let args: Value = serde_json::from_str(json_part).ok()?;
    Some((name, args))
}

/// Parse Llama3/Hermes XML format: `<function=tool_name> <parameter=arg> value </parameter> </function>`
///
/// Qwen3-Coder models sometimes output this format instead of JSON tool calls.
fn try_parse_llama3_xml_format(trimmed: &str) -> Option<(String, Value)> {
    // Match: <function=TOOL_NAME> ... </function>
    let func_start = trimmed.find("<function=")?;
    let func_name_start = func_start + "<function=".len();
    let func_name_end = trimmed[func_name_start..].find('>')? + func_name_start;
    let name = trimmed[func_name_start..func_name_end].trim().to_string();

    if name.is_empty() {
        return None;
    }

    // Extract all <parameter=NAME> VALUE </parameter> pairs
    let mut args = serde_json::Map::new();
    let body = &trimmed[func_name_end + 1..];

    let mut search_pos = 0;
    while let Some(param_start) = body[search_pos..].find("<parameter=") {
        let abs_start = search_pos + param_start;
        let name_start = abs_start + "<parameter=".len();
        let name_end = match body[name_start..].find('>') {
            Some(i) => name_start + i,
            None => break,
        };
        let param_name = body[name_start..name_end].trim().to_string();

        let value_start = name_end + 1;
        let value_end = match body[value_start..].find("</parameter>") {
            Some(i) => value_start + i,
            None => break,
        };
        let param_value = body[value_start..value_end].trim().to_string();

        args.insert(param_name, Value::String(param_value));
        search_pos = value_end + "</parameter>".len();
    }

    Some((name, Value::Object(args)))
}

/// Parse GLM native XML format: `tool_name\n<arg_key>key</arg_key>\n<arg_value>val</arg_value>`
///
/// GLM models can emit this format from their chat template instead of JSON.
/// The function name is on the first line, followed by `<arg_key>`/`<arg_value>` pairs.
fn try_parse_glm_xml_format(trimmed: &str) -> Option<(String, Value)> {
    // Must contain <arg_key> to be this format
    if !trimmed.contains("<arg_key>") {
        return None;
    }

    // Function name is the text before the first <arg_key> (or the whole first line)
    let first_arg_pos = trimmed.find("<arg_key>")?;
    let name = trimmed[..first_arg_pos].trim().to_string();
    if name.is_empty() || name.contains(' ') || name.contains('{') {
        return None;
    }

    // Extract all <arg_key>NAME</arg_key>\n<arg_value>VALUE</arg_value> pairs
    let mut args = serde_json::Map::new();
    let mut search_pos = first_arg_pos;

    while let Some(key_start) = trimmed[search_pos..].find("<arg_key>") {
        let abs_key_start = search_pos + key_start + "<arg_key>".len();
        let key_end = match trimmed[abs_key_start..].find("</arg_key>") {
            Some(i) => abs_key_start + i,
            None => break,
        };
        let key = trimmed[abs_key_start..key_end].trim().to_string();

        // Find the matching <arg_value>
        let after_key = key_end + "</arg_key>".len();
        let val_start = match trimmed[after_key..].find("<arg_value>") {
            Some(i) => after_key + i + "<arg_value>".len(),
            None => break,
        };
        let val_end = match trimmed[val_start..].find("</arg_value>") {
            Some(i) => val_start + i,
            None => break,
        };
        let value = trimmed[val_start..val_end].trim().to_string();

        // Try to parse as JSON first (for non-string values), fall back to string
        let json_value = serde_json::from_str::<Value>(&value)
            .unwrap_or_else(|_| Value::String(value));
        args.insert(key, json_value);

        search_pos = val_end + "</arg_value>".len();
    }

    if args.is_empty() {
        return None;
    }

    Some((name, Value::Object(args)))
}

/// Fallback: infer tool name from bare argument keys.
///
/// Some models (GLM) put just the arguments without the name/arguments wrapper inside
/// SYSTEM.EXEC tags, e.g. `{"command": "ls"}` or `{"path": "/tmp", "content": "hello"}`.
/// We infer the tool name from which keys are present.
fn try_infer_tool_from_bare_args(trimmed: &str) -> Option<(String, Value)> {
    let parsed = try_parse_with_fixups(trimmed)?;
    let obj = parsed.as_object()?;

    // Skip if it has a "name" key — that's the standard wrapper format (handled elsewhere)
    if obj.contains_key("name") {
        return None;
    }

    let name = if obj.contains_key("command") {
        "execute_command"
    } else if obj.contains_key("code") {
        "execute_python"
    } else if obj.contains_key("path") && obj.contains_key("content") {
        "write_file"
    } else if obj.contains_key("query") {
        "web_search"
    } else if obj.contains_key("url") {
        "web_fetch"
    } else if obj.contains_key("path") {
        // Could be read_file or list_directory — default to read_file
        "read_file"
    } else {
        return None;
    };

    Some((name.to_string(), parsed))
}

/// Parse LFM2 (Liquid AI) Python function call syntax.
///
/// Format: `[func_name(key="value", key2="value2")]`
/// Examples:
/// - `[read_file(path="agent-tests/TEST_PLAN.md")]`
/// - `[write_file(path="output.txt", content="Hello world")]`
/// - `[execute_python(code="print(\"hello\")\nresult = func()")]`
///
/// Handles unescaped quotes inside values (common with code parameters) by using
/// the LAST quote before `)` or `, next_key=` as the value terminator.
///
/// Returns `Some((name, args_json))` if parsed.
fn try_parse_lfm2_python_call_format(trimmed: &str) -> Option<(String, Value)> {
    // Must start with '[' and end with ']'
    let inner = trimmed.strip_prefix('[')?.strip_suffix(']')?.trim();

    // Split function name from args: "func_name(args...)"
    let paren_pos = inner.find('(')?;
    let name = &inner[..paren_pos];

    // Validate function name: must be a simple identifier
    if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return None;
    }

    // Extract content between first '(' and LAST ')'.
    // This handles code with ')' inside (like `f.read()`).
    let after_paren = &inner[paren_pos + 1..];
    let last_close = after_paren.rfind(')')?;
    let args_str = after_paren[..last_close].trim();

    if args_str.is_empty() {
        return Some((name.to_string(), serde_json::json!({})));
    }

    // Strategy: find all `key=` boundaries, then extract values between them.
    // This avoids quote-matching issues with unescaped quotes in code.
    let mut key_positions: Vec<(usize, &str)> = Vec::new();
    let mut scan = 0;
    while scan < args_str.len() {
        // Look for pattern: word_chars followed by '=' followed by '"' or "'"
        if let Some(eq_pos) = args_str[scan..].find('=') {
            let abs_eq = scan + eq_pos;
            // Check: char after '=' should be a quote
            let after_eq = abs_eq + 1;
            if after_eq < args_str.len() {
                let next_ch = args_str.as_bytes()[after_eq];
                if next_ch == b'"' || next_ch == b'\'' {
                    // Check: chars before '=' should be a simple identifier
                    let key_start = args_str[..abs_eq].rfind(|c: char| c == ',' || c == ' ' || c == '\n')
                        .map(|p| p + 1)
                        .unwrap_or(0);
                    let key = args_str[key_start..abs_eq].trim();
                    if !key.is_empty() && key.chars().all(|c| c.is_alphanumeric() || c == '_') {
                        key_positions.push((key_start, key));
                    }
                }
            }
            scan = abs_eq + 1;
        } else {
            break;
        }
    }

    if key_positions.is_empty() {
        return None;
    }

    let mut args = serde_json::Map::new();
    for (i, &(key_start, key)) in key_positions.iter().enumerate() {
        // Value starts after `key="`
        let val_open = key_start + key.len() + 1; // position of opening quote
        if val_open >= args_str.len() {
            continue;
        }
        let quote_char = args_str.as_bytes()[val_open];
        let val_content_start = val_open + 1; // position after opening quote

        // Value ends at the last quote before the NEXT key (or end of args_str)
        let val_region_end = if i + 1 < key_positions.len() {
            key_positions[i + 1].0
        } else {
            args_str.len()
        };

        // Find the LAST occurrence of the quote char in this region
        let val_region = &args_str[val_content_start..val_region_end];
        let last_quote = val_region.rfind(quote_char as char);
        let value = match last_quote {
            Some(pos) => &val_region[..pos],
            None => val_region.trim(), // No closing quote found — take everything
        };

        // Unescape common sequences
        let unescaped = value
            .replace("\\n", "\n")
            .replace("\\t", "\t")
            .replace("\\\"", "\"")
            .replace("\\'", "'")
            .replace("\\\\", "\\");

        args.insert(key.to_string(), serde_json::json!(unescaped));
    }

    if args.is_empty() {
        return None;
    }

    Some((name.to_string(), Value::Object(args)))
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
    } else if let Some(result) = try_parse_name_json_format(trimmed) {
        Some(result)
    } else {
        None
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
