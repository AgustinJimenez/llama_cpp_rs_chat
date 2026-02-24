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

/// Escape raw newlines inside JSON string values so serde_json can parse them.
/// Models often emit multiline content like `"content": "line1\nline2"` with literal
/// newlines instead of `\\n`, which is invalid JSON.
fn escape_newlines_in_json_strings(input: &str) -> String {
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
fn auto_close_json(input: &str) -> String {
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
fn escape_invalid_backslashes_in_strings(input: &str) -> String {
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
fn try_parse_with_fixups(input: &str) -> Option<Value> {
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

/// Parse standard JSON format: `{"name":"...","arguments":{...}}` or array `[{...}]`
fn try_parse_json_format(trimmed: &str) -> Option<(String, Value)> {
    let parsed: Value = if trimmed.starts_with('[') {
        let arr = try_parse_with_fixups(trimmed)?;
        arr.as_array()?.first()?.clone()
    } else {
        try_parse_with_fixups(trimmed)?
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

/// Try to parse a tool call text into (name, arguments) using all supported formats.
///
/// Returns `Some((name, args))` if parsed, `None` otherwise.
pub fn try_parse_tool_call(text: &str) -> Option<(String, Value)> {
    let trimmed = text.trim();
    if let Some(result) = try_parse_json_format(trimmed) {
        Some(result)
    } else if let Some(result) = try_parse_mistral_comma_format(trimmed) {
        Some(result)
    } else if let Some(result) = try_parse_llama3_xml_format(trimmed) {
        Some(result)
    } else if let Some(result) = try_parse_name_json_format(trimmed) {
        Some(result)
    } else {
        None
    }
}

/// If the text is an `execute_command` tool call, extract and return the command string.
/// Used by the command executor to route `execute_command` through the streaming path.
pub fn extract_execute_command(text: &str) -> Option<String> {
    // First try the standard tool call format: {"name":"execute_command","arguments":{"command":"..."}}
    if let Some((name, args)) = try_parse_tool_call(text) {
        if name == "execute_command" {
            let command = args.get("command").and_then(|v| v.as_str())?;
            if !command.is_empty() {
                return Some(command.to_string());
            }
        }
        return None;
    }

    // Fallback: some models (GLM) put bare arguments without the name/arguments wrapper,
    // e.g. {"command": "..."} inside SYSTEM.EXEC tags
    let trimmed = text.trim();
    if let Some(parsed) = try_parse_with_fixups(trimmed) {
        if let Some(command) = parsed.get("command").and_then(|v| v.as_str()) {
            if !command.is_empty() {
                return Some(command.to_string());
            }
        }
    }
    None
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
///
/// Note: `execute_command` is handled here as a blocking fallback. The command executor
/// should prefer `extract_execute_command()` + `execute_command_streaming()` for streaming.
pub fn dispatch_native_tool(text: &str, web_search_provider: Option<&str>) -> Option<String> {
    let trimmed = text.trim();

    let (name, args) = if let Some((n, a)) = try_parse_json_format(trimmed) {
        (n, a)
    } else if let Some((n, a)) = try_parse_mistral_comma_format(trimmed) {
        (n, a)
    } else if let Some((n, a)) = try_parse_llama3_xml_format(trimmed) {
        (n, a)
    } else if let Some((n, a)) = try_parse_name_json_format(trimmed) {
        (n, a)
    } else if let Some((n, a)) = try_infer_tool_from_bare_args(trimmed) {
        // Fallback: some models (GLM) put bare arguments without name/arguments wrapper
        (n, a)
    } else {
        return None;
    };

    Some(match name.as_str() {
        "read_file" => tool_read_file(&args),
        "write_file" => tool_write_file(&args),
        "execute_python" => tool_execute_python(&args),
        "list_directory" => tool_list_directory(&args),
        "web_search" => tool_web_search(&args, web_search_provider),
        "web_fetch" => tool_web_fetch(&args),
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
        Err(e) => format!("Error reading '{path}': {e}"),
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
                return format!("Error creating directories for '{path}': {e}");
            }
        }
    }

    match std::fs::write(path, content) {
        Ok(()) => format!("Written {} bytes to {}", content.len(), path),
        Err(e) => format!("Error writing '{path}': {e}"),
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
        return format!("Error writing temp file: {e}");
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
                format!("{stdout}\nStderr: {stderr}")
            } else if stdout.is_empty() {
                "Python script executed successfully (no output)".to_string()
            } else {
                stdout.to_string()
            }
        }
        Err(e) => format!("Error running Python: {e}"),
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
        Err(e) => return format!("Error reading directory '{path}': {e}"),
    };

    let mut lines = Vec::new();
    lines.push(format!("Directory listing: {path}"));
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
        lines.push(format!("{name:<40} {size:>10} {file_type}"));
    }

    lines.join("\n")
}

/// Maximum characters to return from web search results.
const MAX_SEARCH_RESULT_CHARS: usize = 8_000;

/// Maximum characters to return from web page fetch.
const MAX_FETCH_CHARS: usize = 15_000;

/// Search the web using DuckDuckGo Instant Answer API, falling back to HTML scraping.
fn tool_web_search(args: &Value, provider: Option<&str>) -> String {
    let query = match args.get("query").and_then(|v| v.as_str()) {
        Some(q) => q,
        None => return "Error: 'query' argument is required".to_string(),
    };

    let max_results = args
        .get("max_results")
        .and_then(|v| v.as_u64())
        .unwrap_or(8) as usize;

    match provider {
        Some("Google") => {
            eprintln!("[WEB_SEARCH] Using Google via headless Chrome");
            tool_web_search_google_chrome(query, max_results)
        }
        _ => {
            // DuckDuckGo (default)
            // Try DuckDuckGo Instant Answer API first (reliable, no CAPTCHAs)
            match tool_web_search_ddg_api(query, max_results) {
                Some(result) if !result.is_empty() => return result,
                _ => {
                    eprintln!("[WEB_SEARCH] DDG API returned no results, trying HTML scraping");
                }
            }

            // Fallback: ureq-based DuckDuckGo HTML scraping
            tool_web_search_ureq(query, max_results)
        }
    }
}

/// Search using Google via headless Chrome.
/// Navigates to Google search, extracts result titles, snippets, and URLs from the DOM.
fn tool_web_search_google_chrome(query: &str, max_results: usize) -> String {
    let url = format!(
        "https://www.google.com/search?q={}&num={}&hl=en",
        urlencoding::encode(query),
        max_results.min(10)
    );

    // Use Chrome to fetch the search results page
    match super::browser::chrome_web_fetch(&url, 50_000) {
        Ok(content) if !content.is_empty() => {
            // Parse the text output for search results
            // Chrome returns html2text-formatted content from Google's SERP
            let mut output = String::new();
            let mut count = 0;

            // html2text converts Google results into a readable format.
            // We extract lines that look like result titles/URLs/snippets.
            let lines: Vec<&str> = content.lines().collect();
            let mut i = 0;
            while i < lines.len() && count < max_results {
                let line = lines[i].trim();

                // Detect result links: html2text renders [title][url] or title\nurl patterns
                // Google SERP in text form has patterns like:
                // "[Title](https://...)" or just "https://..." lines
                if let Some(link_start) = line.find("](http") {
                    // Markdown-style link: [Title](URL)
                    let title = &line[1..link_start];
                    let url_end = line[link_start + 2..].find(')').unwrap_or(line.len() - link_start - 2);
                    let result_url = &line[link_start + 2..link_start + 2 + url_end];

                    // Skip Google internal links, image links, cached links
                    if !result_url.contains("google.com")
                        && !result_url.contains("webcache")
                        && !title.is_empty()
                        && title.len() > 3
                    {
                        count += 1;
                        output.push_str(&format!("{count}. {title}\n"));
                        output.push_str(&format!("   URL: {result_url}\n"));

                        // Look for a snippet on the next few lines
                        for j in 1..=3 {
                            if i + j < lines.len() {
                                let next = lines[i + j].trim();
                                if !next.is_empty()
                                    && !next.starts_with('[')
                                    && !next.starts_with("http")
                                    && next.len() > 20
                                {
                                    output.push_str(&format!("   {next}\n"));
                                    break;
                                }
                            }
                        }
                        output.push('\n');
                    }
                }
                i += 1;
            }

            if output.is_empty() {
                // Fallback: return raw text content (truncated) if parsing failed
                let truncated = if content.len() > MAX_SEARCH_RESULT_CHARS {
                    format!(
                        "{}...\n[Truncated: {} of {} chars]",
                        &content[..MAX_SEARCH_RESULT_CHARS],
                        MAX_SEARCH_RESULT_CHARS,
                        content.len()
                    )
                } else {
                    content
                };
                return format!(
                    "Search results for '{query}' (via Google):\n\n{truncated}"
                );
            }

            if output.len() > MAX_SEARCH_RESULT_CHARS {
                output.truncate(MAX_SEARCH_RESULT_CHARS);
                output.push_str("\n...[truncated]");
            }

            format!(
                "Search results for '{query}' (via Google):\n\n{output}\
                 Note: Use web_fetch to read specific URLs for more detail."
            )
        }
        Ok(_) => {
            eprintln!("[WEB_SEARCH] Google Chrome returned empty content, falling back to DDG");
            tool_web_search_ureq(query, max_results)
        }
        Err(e) => {
            let e_lower = e.to_lowercase();
            let timed_out = e_lower.contains("timed out") || e_lower.contains("timeout");
            eprintln!("[WEB_SEARCH] Google Chrome failed: {e}, falling back to DDG");
            let result = tool_web_search_ureq(query, max_results);
            if timed_out && (result.contains("No results") || result.starts_with("Error")) {
                format!("Error: Web search timed out for query '{query}'. The search engine did not respond in time. Try again or use a different query.")
            } else {
                result
            }
        }
    }
}

/// Search using DuckDuckGo Instant Answer API (knowledge-based results).
/// Returns structured results from Wikipedia, related topics, and direct answers.
fn tool_web_search_ddg_api(query: &str, max_results: usize) -> Option<String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_read(std::time::Duration::from_secs(10))
        .timeout_connect(std::time::Duration::from_secs(5))
        .user_agent("Mozilla/5.0 (compatible; LlamaChat/1.0)")
        .build();

    let url = format!(
        "https://api.duckduckgo.com/?q={}&format=json&no_html=1&skip_disambig=1",
        urlencoding::encode(query)
    );

    let response = agent.get(&url).call().ok()?;
    let body = response.into_string().ok()?;
    let data: Value = serde_json::from_str(&body).ok()?;

    let mut output = String::new();
    let mut count = 0;

    // Main abstract (usually Wikipedia)
    let abstract_text = data["AbstractText"].as_str().unwrap_or("");
    let abstract_url = data["AbstractURL"].as_str().unwrap_or("");
    let abstract_source = data["AbstractSource"].as_str().unwrap_or("");
    let heading = data["Heading"].as_str().unwrap_or("");

    if !abstract_text.is_empty() {
        count += 1;
        output.push_str(&format!("{count}. {heading}\n"));
        output.push_str(&format!("   Source: {abstract_source}\n"));
        if !abstract_url.is_empty() {
            output.push_str(&format!("   URL: {abstract_url}\n"));
        }
        output.push_str(&format!("   {abstract_text}\n\n"));
    }

    // Direct answer (e.g., calculator, conversions)
    let answer = data["Answer"].as_str().unwrap_or("");
    if !answer.is_empty() {
        count += 1;
        output.push_str(&format!("{count}. Direct Answer\n"));
        output.push_str(&format!("   {answer}\n\n"));
    }

    // Related topics (additional knowledge links)
    if let Some(topics) = data["RelatedTopics"].as_array() {
        for topic in topics {
            if count >= max_results {
                break;
            }
            // Direct topic entry
            if let (Some(text), Some(url)) = (topic["Text"].as_str(), topic["FirstURL"].as_str())
            {
                count += 1;
                output.push_str(&format!("{count}. {text}\n"));
                output.push_str(&format!("   URL: {url}\n\n"));
            }
            // Grouped topics (subcategories)
            if let Some(sub_topics) = topic["Topics"].as_array() {
                for sub in sub_topics {
                    if count >= max_results {
                        break;
                    }
                    if let (Some(text), Some(url)) =
                        (sub["Text"].as_str(), sub["FirstURL"].as_str())
                    {
                        count += 1;
                        output.push_str(&format!("{count}. {text}\n"));
                        output.push_str(&format!("   URL: {url}\n\n"));
                    }
                }
            }
        }
    }

    if output.is_empty() {
        return None;
    }

    if output.len() > MAX_SEARCH_RESULT_CHARS {
        output.truncate(MAX_SEARCH_RESULT_CHARS);
        output.push_str("\n...[truncated]");
    }

    Some(format!(
        "Search results for '{query}' (via DuckDuckGo):\n\n{output}\
         Note: These are knowledge-based results. Use web_fetch to read specific URLs for more detail."
    ))
}

/// Fallback web search via ureq HTTP scraping (used when Chrome is unavailable).
fn tool_web_search_ureq(query: &str, max_results: usize) -> String {
    let agent = ureq::AgentBuilder::new()
        .timeout_read(std::time::Duration::from_secs(15))
        .timeout_connect(std::time::Duration::from_secs(10))
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .build();

    let url = format!(
        "https://html.duckduckgo.com/html/?q={}",
        urlencoding::encode(query)
    );

    let response = match agent.get(&url).call() {
        Ok(r) => r,
        Err(e) => return format!("Error: Failed to search DuckDuckGo: {e}"),
    };

    let body = match response.into_string() {
        Ok(b) => b,
        Err(e) => return format!("Error: Failed to read search response: {e}"),
    };

    // Try structured regex parsing first
    let results = parse_ddg_results(&body, max_results);
    if !results.is_empty() {
        return format!("Search results for '{query}':\n\n{results}");
    }

    // Fallback: use html2text for raw conversion
    let text = html2text::from_read(body.as_bytes(), 120);
    let truncated = if text.len() > MAX_SEARCH_RESULT_CHARS {
        format!(
            "{}...\n[Truncated: first {} of {} chars]",
            &text[..MAX_SEARCH_RESULT_CHARS],
            MAX_SEARCH_RESULT_CHARS,
            text.len()
        )
    } else {
        text
    };

    format!("Search results for '{query}':\n\n{truncated}")
}

/// Parse DuckDuckGo HTML search results into structured text.
fn parse_ddg_results(html: &str, max_results: usize) -> String {
    use regex::Regex;

    lazy_static::lazy_static! {
        static ref RESULT_LINK: Regex = Regex::new(
            r#"(?s)class="result__a"[^>]*href="([^"]*)"[^>]*>([^<]*)</a>"#
        ).unwrap();

        static ref RESULT_SNIPPET: Regex = Regex::new(
            r#"(?s)class="result__snippet"[^>]*>(.*?)</(?:a|td|div|span)"#
        ).unwrap();
    }

    let links: Vec<(String, String)> = RESULT_LINK
        .captures_iter(html)
        .take(max_results)
        .map(|cap| {
            let href = cap.get(1).map(|m| m.as_str()).unwrap_or("");
            let title = cap
                .get(2)
                .map(|m| m.as_str().trim())
                .unwrap_or("")
                .replace("&amp;", "&")
                .replace("&lt;", "<")
                .replace("&gt;", ">")
                .replace("&quot;", "\"")
                .replace("&#x27;", "'");
            (href.to_string(), title)
        })
        .collect();

    if links.is_empty() {
        return String::new();
    }

    let snippets: Vec<String> = RESULT_SNIPPET
        .captures_iter(html)
        .take(max_results)
        .map(|cap| {
            let raw = cap.get(1).map(|m| m.as_str()).unwrap_or("");
            // Strip inner HTML tags from snippet
            let tag_re = regex::Regex::new(r"<[^>]+>").unwrap();
            tag_re
                .replace_all(raw, "")
                .trim()
                .replace("&amp;", "&")
                .replace("&lt;", "<")
                .replace("&gt;", ">")
                .replace("&quot;", "\"")
                .replace("&#x27;", "'")
        })
        .collect();

    let mut output = String::new();
    for (i, (href, title)) in links.iter().enumerate() {
        let snippet = snippets.get(i).map(|s| s.as_str()).unwrap_or("");

        output.push_str(&format!("{}. {title}\n", i + 1));
        output.push_str(&format!("   URL: {href}\n"));
        if !snippet.is_empty() {
            output.push_str(&format!("   {snippet}\n"));
        }
        output.push('\n');
    }

    if output.len() > MAX_SEARCH_RESULT_CHARS {
        output.truncate(MAX_SEARCH_RESULT_CHARS);
        output.push_str("\n...[truncated]");
    }

    output
}

/// Fetch a web page using headless Chrome (JS-rendered), falling back to ureq.
fn tool_web_fetch(args: &Value) -> String {
    let url = match args.get("url").and_then(|v| v.as_str()) {
        Some(u) => u,
        None => return "Error: 'url' argument is required".to_string(),
    };

    let max_chars = args
        .get("max_length")
        .and_then(|v| v.as_u64())
        .unwrap_or(MAX_FETCH_CHARS as u64) as usize;

    // Try headless Chrome first (gets JS-rendered content)
    let chrome_timed_out = match super::browser::chrome_web_fetch(url, max_chars) {
        Ok(content) if !content.is_empty() => return content,
        Ok(_) => {
            eprintln!("[WEB_FETCH] Chrome returned empty content, falling back to ureq");
            false
        }
        Err(e) => {
            let e_lower = e.to_lowercase();
            let timed_out = e_lower.contains("timed out") || e_lower.contains("timeout");
            eprintln!("[WEB_FETCH] Chrome failed: {e}, falling back to ureq");
            timed_out
        }
    };

    // Fallback: ureq-based fetch
    let result = tool_web_fetch_ureq(url, max_chars);

    // If both Chrome and ureq failed, prepend a clear timeout notice
    if chrome_timed_out && result.starts_with("Error") {
        format!("Error: Request timed out. The URL '{url}' did not respond within the timeout period. {result}")
    } else {
        result
    }
}

/// Fallback web fetch via ureq HTTP client (used when Chrome is unavailable).
fn tool_web_fetch_ureq(url: &str, max_chars: usize) -> String {
    let result = crate::web::routes::tools::fetch_url_as_text(url, max_chars);

    if let Some(true) = result.get("success").and_then(|v| v.as_bool()) {
        result
            .get("result")
            .and_then(|v| v.as_str())
            .unwrap_or("(empty page)")
            .to_string()
    } else {
        let error = result
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown error");
        format!("Error fetching URL: {error}")
    }
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
        let result = dispatch_native_tool(&json, None);
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
        let result = dispatch_native_tool(&json, None);
        assert!(result.is_some());
        assert!(result.unwrap().contains("Written"));
        assert_eq!(std::fs::read_to_string(&temp).unwrap(), "test content");

        std::fs::remove_file(&temp).ok();
    }

    #[test]
    fn test_dispatch_write_file_multiline_json_content() {
        // Models often emit multiline JSON content with literal newlines
        let temp = std::env::temp_dir().join("native_tools_test_multiline.json");
        let json = format!(
            "{{\n  \"name\": \"write_file\",\n  \"arguments\": {{\n    \"path\": \"{}\",\n    \"content\": \"{{\n  \\\"name\\\": \\\"test\\\",\n  \\\"version\\\": \\\"1.0.0\\\"\n}}\"\n  }}\n}}",
            temp.display().to_string().replace('\\', "\\\\")
        );
        let result = dispatch_native_tool(&json, None);
        assert!(result.is_some(), "Should parse multiline JSON content: {json}");
        assert!(result.unwrap().contains("Written"));
        let content = std::fs::read_to_string(&temp).unwrap();
        assert!(content.contains("\"name\": \"test\""));
        std::fs::remove_file(&temp).ok();
    }

    #[test]
    fn test_escape_newlines_in_json_strings() {
        let input = r#"{"name": "write_file", "arguments": {"content": "line1
line2
line3"}}"#;
        let escaped = escape_newlines_in_json_strings(input);
        let parsed: Value = serde_json::from_str(&escaped).unwrap();
        let content = parsed["arguments"]["content"].as_str().unwrap();
        assert_eq!(content, "line1\nline2\nline3");
    }

    #[test]
    fn test_dispatch_list_directory() {
        let json = r#"{"name": "list_directory", "arguments": {"path": "."}}"#;
        let result = dispatch_native_tool(json, None);
        assert!(result.is_some());
        assert!(result.unwrap().contains("Directory listing"));
    }

    #[test]
    fn test_dispatch_unknown_tool_returns_none() {
        let json = r#"{"name": "unknown_tool", "arguments": {}}"#;
        let result = dispatch_native_tool(json, None);
        assert!(result.is_none());
    }

    #[test]
    fn test_dispatch_non_json_returns_none() {
        let result = dispatch_native_tool("ls -la", None);
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
        let result = dispatch_native_tool(&json, None);
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
        let result = dispatch_native_tool(&input, None);
        assert!(result.is_some(), "Should parse Mistral comma format");
        assert!(result.unwrap().contains("comma format test"));

        std::fs::remove_file(&temp).ok();
    }

    #[test]
    fn test_dispatch_mistral_comma_execute_command() {
        // Devstral: execute_command,{"command": "echo hello"}
        let input = r#"execute_command,{"command": "echo hello"}"#;
        let result = dispatch_native_tool(input, None);
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
        let result = dispatch_native_tool(&input, None);
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
        let result = dispatch_native_tool(&input, None);
        assert!(result.is_some(), "Should parse Llama3 XML write_file");
        assert!(result.unwrap().contains("Written"));
        assert_eq!(std::fs::read_to_string(&temp).unwrap(), "hello world");

        std::fs::remove_file(&temp).ok();
    }

    #[test]
    fn test_dispatch_name_json_format() {
        // Granite outputs: list_directory{"path": "."}
        let input = r#"list_directory{"path": "."}"#;
        let result = dispatch_native_tool(input, None);
        assert!(result.is_some(), "Should parse name+JSON format");
        assert!(result.unwrap().contains("Directory listing"));
    }

    #[test]
    fn test_execute_python_simple() {
        let json = r#"{"name": "execute_python", "arguments": {"code": "print('hello from python')"}}"#;
        let result = dispatch_native_tool(json, None);
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

    #[test]
    fn test_auto_close_json_missing_brace() {
        // GLM model pattern: emits write_file JSON missing the outer closing }
        let input = r#"{"name": "write_file", "arguments": {"path": "/tmp/test.txt", "content": "hello"}}"#;
        // Valid JSON - should parse fine
        assert!(serde_json::from_str::<Value>(input).is_ok());

        // Now remove the last } to simulate GLM's bug
        let broken = &input[..input.len() - 1]; // {"name": "write_file", "arguments": {"path": "/tmp/test.txt", "content": "hello"}}  -> missing last }
        assert!(serde_json::from_str::<Value>(broken).is_err());

        let fixed = auto_close_json(broken);
        assert_eq!(fixed, input); // Should add back the missing }
        assert!(serde_json::from_str::<Value>(&fixed).is_ok());
    }

    #[test]
    fn test_dispatch_write_file_missing_brace_with_newlines() {
        // Exact pattern GLM produces: multiline content + missing outer closing }
        let json = "{\"name\": \"write_file\", \"arguments\": {\"path\": \"/tmp/test-autoclose.txt\", \"content\": \"{\n  \\\"name\\\": \\\"test\\\",\n  \\\"version\\\": \\\"1.0.0\\\"\n}\"}}";
        // This should work (has both braces)
        let result = dispatch_native_tool(json, None);
        assert!(result.is_some(), "Valid JSON should work: {:?}", result);
        let _ = std::fs::remove_file("/tmp/test-autoclose.txt");

        // Now test with missing outer brace (GLM pattern)
        let broken = "{\"name\": \"write_file\", \"arguments\": {\"path\": \"/tmp/test-autoclose2.txt\", \"content\": \"{\n  \\\"name\\\": \\\"test\\\",\n  \\\"version\\\": \\\"1.0.0\\\"\n}\"}}";
        // Remove last }
        let broken = &broken[..broken.len() - 1];
        let result = dispatch_native_tool(broken, None);
        assert!(result.is_some(), "Should auto-close missing brace and dispatch write_file");
        let output = result.unwrap();
        assert!(output.contains("written") || output.contains("success") || output.contains("Written"),
            "Should write successfully: {}", output);
        let _ = std::fs::remove_file("/tmp/test-autoclose2.txt");
    }

    #[test]
    fn test_escape_invalid_backslashes_php_namespaces() {
        // PHP namespaces like Illuminate\Database produce \D which is invalid JSON escape
        let input = r#"{"name":"write_file","arguments":{"path":"Person.php","content":"namespace App\Models;\nuse Illuminate\Database\Eloquent\Model;"}}"#;
        let fixed = escape_invalid_backslashes_in_strings(input);
        // Should double the backslashes before invalid escape chars (M, D, E)
        assert!(fixed.contains(r"App\\Models"));
        assert!(fixed.contains(r"Illuminate\\Database\\Eloquent\\Model"));
        // Should now parse as valid JSON
        assert!(serde_json::from_str::<Value>(&fixed).is_ok(), "Fixed JSON should parse: {}", fixed);
    }

    #[test]
    fn test_escape_invalid_backslashes_preserves_valid_escapes() {
        // Valid JSON escapes should NOT be doubled
        let input = r#"{"content":"line1\nline2\ttab\"quoted\\"}"#;
        let fixed = escape_invalid_backslashes_in_strings(input);
        assert_eq!(input, fixed, "Valid escapes should be unchanged");
    }

    #[test]
    fn test_dispatch_write_file_php_namespaces() {
        // End-to-end: dispatch_native_tool should handle PHP namespaces via fixup chain
        let temp = std::env::temp_dir().join("native_tools_test_php_ns.php");
        let json = format!(
            r#"{{"name":"write_file","arguments":{{"path":"{}","content":"<?php\nnamespace App\Models;\nuse Illuminate\Database\Eloquent\Model;\n\nclass Person extends Model {{\n    protected $fillable = ['name'];\n}}"}}}}"#,
            temp.display()
        );
        let result = dispatch_native_tool(&json, None);
        assert!(result.is_some(), "Should parse PHP namespace JSON via fixup chain");
        let output = result.unwrap();
        assert!(output.contains("Written"), "Should write file: {}", output);

        let content = std::fs::read_to_string(&temp).unwrap();
        assert!(content.contains(r"App\Models"), "Should preserve single backslash in file content");
        assert!(content.contains(r"Illuminate\Database\Eloquent\Model"), "Should preserve namespace path");

        std::fs::remove_file(&temp).ok();
    }

    #[test]
    fn test_parse_ddg_results_extracts_links_and_snippets() {
        let html = r#"
        <div class="result">
            <a class="result__a" href="https://example.com/page1">Example Page One</a>
            <td class="result__snippet">This is the first result snippet about example.</td>
        </div>
        <div class="result">
            <a class="result__a" href="https://example.com/page2">Example &amp; Page Two</a>
            <td class="result__snippet">Second result with <b>bold</b> text.</td>
        </div>
        "#;

        let result = parse_ddg_results(html, 10);
        assert!(result.contains("Example Page One"), "Should extract first title");
        assert!(result.contains("https://example.com/page1"), "Should extract first URL");
        assert!(result.contains("first result snippet"), "Should extract first snippet");
        assert!(result.contains("Example & Page Two"), "Should decode &amp;");
        assert!(result.contains("https://example.com/page2"), "Should extract second URL");
        assert!(result.contains("Second result with"), "Should extract second snippet");
        assert!(!result.contains("<b>"), "Should strip inner HTML tags from snippets");
    }

    #[test]
    fn test_parse_ddg_results_empty_html() {
        let result = parse_ddg_results("<html><body>no results</body></html>", 10);
        assert!(result.is_empty(), "Should return empty string for no results");
    }

    #[test]
    fn test_dispatch_web_search_missing_query() {
        let json = r#"{"name": "web_search", "arguments": {}}"#;
        let result = dispatch_native_tool(json, None);
        assert!(result.is_some());
        assert!(result.unwrap().contains("Error"));
    }

    #[test]
    fn test_dispatch_web_fetch_missing_url() {
        let json = r#"{"name": "web_fetch", "arguments": {}}"#;
        let result = dispatch_native_tool(json, None);
        assert!(result.is_some());
        assert!(result.unwrap().contains("Error"));
    }

    #[test]
    fn test_ddg_api_formats_output() {
        // Test that tool_web_search_ddg_api returns formatted output for known queries
        // This is an integration test that calls the real API
        let result = tool_web_search_ddg_api("rust programming language", 5);
        assert!(result.is_some(), "DDG API should return results for 'rust programming language'");
        let text = result.unwrap();
        assert!(text.contains("Rust"), "Should contain 'Rust' in results");
        assert!(text.contains("URL:"), "Should contain URLs");
        assert!(text.contains("Search results for"), "Should have header");
    }
}
