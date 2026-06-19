use serde_json::Value;

fn extract_name_args(obj: &Value) -> Option<(String, Value)> {
    let name = obj.get("name")?.as_str()?.to_string();
    let args = obj
        .get("arguments")
        .cloned()
        .unwrap_or(Value::Object(serde_json::Map::new()));
    Some((name, args))
}

pub(super) fn try_parse_json_format(trimmed: &str) -> Option<(String, Value)> {
    try_parse_all_json_calls(trimmed).and_then(|v| v.into_iter().next())
}

pub(super) fn try_parse_all_json_calls(trimmed: &str) -> Option<Vec<(String, Value)>> {
    let parsed = super::try_parse_with_fixups(trimmed)?;

    if let Some(arr) = parsed.as_array() {
        let calls: Vec<(String, Value)> =
            arr.iter().filter_map(extract_name_args).collect();
        if calls.is_empty() {
            None
        } else {
            Some(calls)
        }
    } else {
        let call = extract_name_args(&parsed)?;
        Some(vec![call])
    }
}

pub(super) fn try_parse_mistral_comma_format(trimmed: &str) -> Option<(String, Value)> {
    let comma_idx = trimmed.find(",{")?;
    let name = trimmed[..comma_idx].trim().to_string();
    let json_part = &trimmed[comma_idx + 1..];
    let args: Value = serde_json::from_str(json_part).ok()?;

    if name.is_empty() || name.contains(' ') || name.contains('{') {
        return None;
    }
    Some((name, args))
}

pub(super) fn try_parse_name_json_format(trimmed: &str) -> Option<(String, Value)> {
    let brace_idx = trimmed.find('{')?;
    let name = trimmed[..brace_idx].trim().to_string();
    let json_part = &trimmed[brace_idx..];

    if name.is_empty() || name.contains(' ') || name.contains('<') || name.contains('>') {
        return None;
    }

    let args: Value = serde_json::from_str(json_part).ok()?;
    Some((name, args))
}

pub(super) fn try_parse_llama3_xml_format(trimmed: &str) -> Option<(String, Value)> {
    let func_start = trimmed.find("<function=")?;
    let func_name_start = func_start + "<function=".len();
    let func_name_end = trimmed[func_name_start..].find('>')? + func_name_start;
    let name = trimmed[func_name_start..func_name_end].trim().to_string();

    if name.is_empty() {
        return None;
    }

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

pub(super) fn try_parse_glm_xml_format(trimmed: &str) -> Option<(String, Value)> {
    if !trimmed.contains("<arg_key>") {
        return None;
    }

    let first_arg_pos = trimmed.find("<arg_key>")?;
    let name = trimmed[..first_arg_pos].trim().to_string();
    if name.is_empty() || name.contains(' ') || name.contains('{') {
        return None;
    }

    let mut args = serde_json::Map::new();
    let mut search_pos = first_arg_pos;

    while let Some(key_start) = trimmed[search_pos..].find("<arg_key>") {
        let abs_key_start = search_pos + key_start + "<arg_key>".len();
        let key_end = match trimmed[abs_key_start..].find("</arg_key>") {
            Some(i) => abs_key_start + i,
            None => break,
        };
        let key = trimmed[abs_key_start..key_end].trim().to_string();
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
        let json_value =
            serde_json::from_str::<Value>(&value).unwrap_or(Value::String(value));
        args.insert(key, json_value);

        search_pos = val_end + "</arg_value>".len();
    }

    if args.is_empty() {
        return None;
    }

    Some((name, Value::Object(args)))
}

pub(super) fn try_infer_tool_from_bare_args(trimmed: &str) -> Option<(String, Value)> {
    let parsed = super::try_parse_with_fixups(trimmed)?;
    let obj = parsed.as_object()?;
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
        "browser_search"
    } else if obj.contains_key("url") {
        "browser_navigate"
    } else if obj.contains_key("path") {
        "read_file"
    } else {
        return None;
    };

    Some((name.to_string(), parsed))
}

pub(super) fn try_parse_lfm2_python_call_format(trimmed: &str) -> Option<(String, Value)> {
    let inner = trimmed.strip_prefix('[')?.strip_suffix(']')?.trim();
    let paren_pos = inner.find('(')?;
    let name = &inner[..paren_pos];

    if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return None;
    }

    let after_paren = &inner[paren_pos + 1..];
    let last_close = after_paren.rfind(')')?;
    let args_str = after_paren[..last_close].trim();
    if args_str.is_empty() {
        return Some((name.to_string(), serde_json::json!({})));
    }

    let mut key_positions: Vec<(usize, &str)> = Vec::new();
    let mut scan = 0;
    while scan < args_str.len() {
        if let Some(eq_pos) = args_str[scan..].find('=') {
            let abs_eq = scan + eq_pos;
            let after_eq = abs_eq + 1;
            if after_eq < args_str.len() {
                let next_ch = args_str.as_bytes()[after_eq];
                if next_ch == b'"' || next_ch == b'\'' {
                    let key_start = args_str[..abs_eq]
                        .rfind([',', ' ', '\n'])
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
        let val_open = key_start + key.len() + 1;
        if val_open >= args_str.len() {
            continue;
        }
        let quote_char = args_str.as_bytes()[val_open];
        let val_content_start = val_open + 1;
        let val_region_end = if i + 1 < key_positions.len() {
            key_positions[i + 1].0
        } else {
            args_str.len()
        };
        let val_region = &args_str[val_content_start..val_region_end];
        let last_quote = val_region.rfind(quote_char as char);
        let value = match last_quote {
            Some(pos) => &val_region[..pos],
            None => val_region.trim(),
        };

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
