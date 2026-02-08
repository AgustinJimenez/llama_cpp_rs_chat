//! Native file I/O and code execution tools.
//!
//! Provides safe, shell-free implementations of common operations that LLM agents
//! need: reading/writing files, running Python code, and listing directories.
//! This eliminates shell quoting issues that break `python -c "..."` on Windows.

use serde_json::Value;
use std::path::Path;
use std::process::Command;

/// Maximum file size to return inline (100 KB).
const MAX_READ_SIZE: usize = 100 * 1024;

/// Parse standard JSON format: `{"name":"...","arguments":{...}}` or array `[{...}]`
fn try_parse_json_format(trimmed: &str) -> Option<(String, Value)> {
    let parsed: Value = if trimmed.starts_with('[') {
        let arr: Value = serde_json::from_str(trimmed).ok()?;
        arr.as_array()?.first()?.clone()
    } else {
        serde_json::from_str(trimmed).ok()?
    };

    let name = parsed.get("name")?.as_str()?.to_string();
    let args = parsed
        .get("arguments")
        .cloned()
        .unwrap_or(Value::Object(serde_json::Map::new()));
    Some((name, args))
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

/// Try to dispatch a tool call to a native handler.
///
/// Supports multiple formats:
/// 1. Standard JSON: `{"name": "read_file", "arguments": {"path": "..."}}`
/// 2. Mistral array:  `[{"name":"read_file","arguments":{"path":"..."}}]`
/// 3. Mistral comma:  `read_file,{"path": "..."}` (Devstral native format)
/// 4. Llama3 XML:     `<function=read_file> <parameter=path> value </parameter> </function>`
/// 5. Name+JSON:      `read_file{"path": "..."}` (Granite native format)
///
/// Returns `Some(output)` if recognized, `None` to fall back to shell.
pub fn dispatch_native_tool(text: &str) -> Option<String> {
    let trimmed = text.trim();

    let (name, args) = if let Some((n, a)) = try_parse_json_format(trimmed) {
        (n, a)
    } else if let Some((n, a)) = try_parse_mistral_comma_format(trimmed) {
        (n, a)
    } else if let Some((n, a)) = try_parse_llama3_xml_format(trimmed) {
        (n, a)
    } else if let Some((n, a)) = try_parse_name_json_format(trimmed) {
        (n, a)
    } else {
        return None;
    };

    Some(match name.as_str() {
        "read_file" => tool_read_file(&args),
        "write_file" => tool_write_file(&args),
        "execute_python" => tool_execute_python(&args),
        "list_directory" => tool_list_directory(&args),
        "execute_command" => {
            // Delegate to shell execution via command.rs
            let command = args.get("command").and_then(|v| v.as_str()).unwrap_or("");
            if command.is_empty() {
                return Some("Error: 'command' argument is required".to_string());
            }
            super::command::execute_command(command)
        }
        _ => return None, // Unknown tool → fall back to shell
    })
}

/// Read a file and return its contents.
fn tool_read_file(args: &Value) -> String {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return "Error: 'path' argument is required".to_string(),
    };

    match std::fs::read_to_string(path) {
        Ok(content) => {
            let total_bytes = content.len();
            if total_bytes > MAX_READ_SIZE {
                format!(
                    "{}\n\n[Truncated: showing first {} of {} bytes]",
                    &content[..MAX_READ_SIZE],
                    MAX_READ_SIZE,
                    total_bytes
                )
            } else {
                content
            }
        }
        Err(e) => format!("Error reading '{}': {}", path, e),
    }
}

/// Write content to a file, creating parent directories as needed.
fn tool_write_file(args: &Value) -> String {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return "Error: 'path' argument is required".to_string(),
    };
    let content = match args.get("content").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return "Error: 'content' argument is required".to_string(),
    };

    // Create parent directories if they don't exist
    if let Some(parent) = Path::new(path).parent() {
        if !parent.exists() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return format!("Error creating directories for '{}': {}", path, e);
            }
        }
    }

    match std::fs::write(path, content) {
        Ok(()) => format!("Written {} bytes to {}", content.len(), path),
        Err(e) => format!("Error writing '{}': {}", path, e),
    }
}

/// Execute Python code by writing to a temp file and running it.
/// This completely bypasses shell quoting — the code goes directly to a .py file.
fn tool_execute_python(args: &Value) -> String {
    let code = match args.get("code").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return "Error: 'code' argument is required".to_string(),
    };

    // Write code to a temp file
    let temp_dir = std::env::temp_dir();
    let temp_file = temp_dir.join(format!(
        "llama_tool_{}.py",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));

    if let Err(e) = std::fs::write(&temp_file, code) {
        return format!("Error writing temp file: {}", e);
    }

    // Run python on the temp file — no shell involved
    let result = Command::new("python")
        .arg(&temp_file)
        .output();

    // Clean up temp file
    let _ = std::fs::remove_file(&temp_file);

    match result {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stderr.is_empty() {
                format!("{}\nStderr: {}", stdout, stderr)
            } else if stdout.is_empty() {
                "Python script executed successfully (no output)".to_string()
            } else {
                stdout.to_string()
            }
        }
        Err(e) => format!("Error running Python: {}", e),
    }
}

/// List directory contents with name, size, and type.
fn tool_list_directory(args: &Value) -> String {
    let path = args
        .get("path")
        .and_then(|v| v.as_str())
        .unwrap_or(".");

    let entries = match std::fs::read_dir(path) {
        Ok(entries) => entries,
        Err(e) => return format!("Error reading directory '{}': {}", path, e),
    };

    let mut lines = Vec::new();
    lines.push(format!("Directory listing: {}", path));
    lines.push(format!("{:<40} {:>10} {}", "Name", "Size", "Type"));
    lines.push("-".repeat(60));

    let mut sorted: Vec<_> = entries.filter_map(|e| e.ok()).collect();
    sorted.sort_by_key(|e| e.file_name());

    for entry in sorted {
        let name = entry.file_name().to_string_lossy().to_string();
        let metadata = entry.metadata();
        let (size, file_type) = match metadata {
            Ok(m) => {
                let ft = if m.is_dir() {
                    "<DIR>"
                } else if m.is_symlink() {
                    "<LINK>"
                } else {
                    "<FILE>"
                };
                (m.len(), ft)
            }
            Err(_) => (0, "<?>"),
        };
        lines.push(format!("{:<40} {:>10} {}", name, size, file_type));
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_dispatch_read_file_valid() {
        // Create a temp file to read
        let temp = std::env::temp_dir().join("native_tools_test_read.txt");
        std::fs::write(&temp, "hello world").unwrap();

        let json = format!(
            r#"{{"name": "read_file", "arguments": {{"path": "{}"}}}}"#,
            temp.display().to_string().replace('\\', "\\\\")
        );
        let result = dispatch_native_tool(&json);
        assert!(result.is_some());
        assert!(result.unwrap().contains("hello world"));

        std::fs::remove_file(&temp).ok();
    }

    #[test]
    fn test_dispatch_write_file() {
        let temp = std::env::temp_dir().join("native_tools_test_write.txt");
        let json = format!(
            r#"{{"name": "write_file", "arguments": {{"path": "{}", "content": "test content"}}}}"#,
            temp.display().to_string().replace('\\', "\\\\")
        );
        let result = dispatch_native_tool(&json);
        assert!(result.is_some());
        assert!(result.unwrap().contains("Written"));
        assert_eq!(std::fs::read_to_string(&temp).unwrap(), "test content");

        std::fs::remove_file(&temp).ok();
    }

    #[test]
    fn test_dispatch_list_directory() {
        let json = r#"{"name": "list_directory", "arguments": {"path": "."}}"#;
        let result = dispatch_native_tool(json);
        assert!(result.is_some());
        assert!(result.unwrap().contains("Directory listing"));
    }

    #[test]
    fn test_dispatch_unknown_tool_returns_none() {
        let json = r#"{"name": "unknown_tool", "arguments": {}}"#;
        let result = dispatch_native_tool(json);
        assert!(result.is_none());
    }

    #[test]
    fn test_dispatch_non_json_returns_none() {
        let result = dispatch_native_tool("ls -la");
        assert!(result.is_none());
    }

    #[test]
    fn test_dispatch_mistral_array_format() {
        let temp = std::env::temp_dir().join("native_tools_test_mistral.txt");
        std::fs::write(&temp, "mistral test").unwrap();

        let json = format!(
            r#"[{{"name": "read_file", "arguments": {{"path": "{}"}}}}]"#,
            temp.display().to_string().replace('\\', "\\\\")
        );
        let result = dispatch_native_tool(&json);
        assert!(result.is_some());
        assert!(result.unwrap().contains("mistral test"));

        std::fs::remove_file(&temp).ok();
    }

    #[test]
    fn test_dispatch_mistral_comma_format() {
        // Devstral outputs: read_file,{"path": "file.txt"}
        let temp = std::env::temp_dir().join("native_tools_test_comma.txt");
        std::fs::write(&temp, "comma format test").unwrap();

        let input = format!(
            r#"read_file,{{"path": "{}"}}"#,
            temp.display().to_string().replace('\\', "\\\\")
        );
        let result = dispatch_native_tool(&input);
        assert!(result.is_some(), "Should parse Mistral comma format");
        assert!(result.unwrap().contains("comma format test"));

        std::fs::remove_file(&temp).ok();
    }

    #[test]
    fn test_dispatch_mistral_comma_execute_command() {
        // Devstral: execute_command,{"command": "echo hello"}
        let input = r#"execute_command,{"command": "echo hello"}"#;
        let result = dispatch_native_tool(input);
        assert!(result.is_some(), "Should parse comma format execute_command");
        assert!(result.unwrap().contains("hello"));
    }

    #[test]
    fn test_dispatch_llama3_xml_format() {
        // Qwen3-Coder outputs: <function=read_file> <parameter=path> file.txt </parameter> </function>
        let temp = std::env::temp_dir().join("native_tools_test_xml.txt");
        std::fs::write(&temp, "xml format test").unwrap();

        let input = format!(
            "<function=read_file> <parameter=path> {} </parameter> </function>",
            temp.display()
        );
        let result = dispatch_native_tool(&input);
        assert!(result.is_some(), "Should parse Llama3 XML format");
        assert!(result.unwrap().contains("xml format test"));

        std::fs::remove_file(&temp).ok();
    }

    #[test]
    fn test_dispatch_llama3_xml_write_file() {
        let temp = std::env::temp_dir().join("native_tools_test_xml_write.txt");
        let input = format!(
            "<function=write_file> <parameter=path> {} </parameter> <parameter=content> hello world </parameter> </function>",
            temp.display()
        );
        let result = dispatch_native_tool(&input);
        assert!(result.is_some(), "Should parse Llama3 XML write_file");
        assert!(result.unwrap().contains("Written"));
        assert_eq!(std::fs::read_to_string(&temp).unwrap(), "hello world");

        std::fs::remove_file(&temp).ok();
    }

    #[test]
    fn test_dispatch_name_json_format() {
        // Granite outputs: list_directory{"path": "."}
        let input = r#"list_directory{"path": "."}"#;
        let result = dispatch_native_tool(input);
        assert!(result.is_some(), "Should parse name+JSON format");
        assert!(result.unwrap().contains("Directory listing"));
    }

    #[test]
    fn test_execute_python_simple() {
        let json = r#"{"name": "execute_python", "arguments": {"code": "print('hello from python')"}}"#;
        let result = dispatch_native_tool(json);
        assert!(result.is_some());
        let output = result.unwrap();
        // If python is available, should contain the output; if not, should contain an error
        assert!(output.contains("hello from python") || output.contains("Error"));
    }

    #[test]
    fn test_execute_python_with_quotes_and_regex() {
        // This is the exact scenario that breaks with shell execution
        let code = r#"import re
text = "Invoice INV-2024-0847 total $1,234.56"
match = re.search(r'\$[\d,]+\.\d+', text)
print(f"Found: {match.group()}" if match else "No match")"#;

        let args = json!({"code": code});
        let result = tool_execute_python(&args);
        // If python is available
        if !result.contains("Error running Python") {
            assert!(result.contains("Found: $1,234.56"));
        }
    }
}
