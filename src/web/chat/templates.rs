use super::jinja_templates::{
    apply_native_chat_template, get_available_tools_openai_with_mcp, parse_conversation_for_jinja,
};
use super::tool_tags::ToolTags;
use super::super::mcp::McpToolDef;

/// Format the current date/time for system prompt injection.
fn current_datetime_string() -> String {
    let now = chrono::Local::now();
    now.format("%Y-%m-%d %H:%M (%A)").to_string()
}

/// Get environment info block shared by all prompt variants.
fn env_block() -> (String, String, &'static str) {
    let os_name = std::env::consts::OS.to_string();
    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| ".".to_string());
    let shell: &str = if os_name == "windows" { "cmd/powershell" } else { "bash" };
    (os_name, cwd, shell)
}

/// Core behavioral instructions shared across all prompt types.
/// Returns the behavior + research + tool usage guidance text.
fn core_behavior_block() -> String {
    r#"## Behavior
- Be autonomous and resourceful. Complete tasks fully without asking the user for help.
- Before installing anything, CHECK if it's already installed (use `where`, `which`, or search common paths like `C:\Users\`, `C:\Program Files\`, `~/.local/bin`).
- If a command fails, try a DIFFERENT approach. Do NOT retry the same failing command.
- Do NOT tell the user to run commands manually — use your tools to solve problems yourself.
- NEVER repeat the same failing command more than once. If it failed, change your approach.
- NEVER output file contents as text in your response. ALWAYS use `write_file` to save code/config/HTML to disk. If you write code without using write_file, IT WILL NOT BE SAVED.
- Use `edit_file` for small changes to existing files instead of rewriting with `write_file`.
- When a task requires multiple steps, execute them one by one using your tools. Do not skip steps.
- For complex tasks, briefly outline your plan before starting.
- When you finish a task, ALWAYS write a brief summary of what you did. Never end on raw tool output.

## Tool Usage Guidelines
- Use `read_file` instead of cat/type. Use `write_file` for new files, `edit_file` for modifications.
- Use `insert_text` to add lines at a specific position (imports, new functions).
- Use `undo_edit` to revert a bad edit_file operation.
- Use `search_files` instead of grep/findstr. Use `find_files` instead of find/dir.
- Use `list_directory` instead of ls/dir.
- Use `execute_python` for Python code (avoids shell quoting issues).
- Use `execute_command` for shell tools (npm, git, etc.).
- Use `git_status`, `git_diff`, `git_commit` for git operations instead of `execute_command`.
- **Web browsing**: Use `browser_search` to search the web (returns Google results). Use `browser_navigate` to open pages, `browser_get_text` to read content, `browser_query` to extract structured data. Do NOT use curl, wget, execute_command, or urllib to fetch web pages — the browser tools use a real browser that bypasses bot detection.
- **Browsing tips**: Avoid JS-heavy sites like Google News or Twitter — use `browser_search` or Google Search instead. If a page returns 404/paywall/empty, try a different source immediately. Use the `summary` parameter with a custom prompt to save tokens on large pages.
- Use `open_url` ONLY when the user explicitly asks to open a page in their external/default browser outside the app. Never use `open_url` for normal browsing, search, page reading, or screenshots.
- Use `take_screenshot` to see the user's screen. Use `click_screen`, `type_text`, `press_key` for desktop automation.
- If a tool is not in PATH, use its full path (e.g., `C:\php\php.exe`) or download it and reference by full path.

## Background Processes
- The `"background"` flag is REQUIRED for execute_command. Set true for servers/daemons (php artisan serve, npm run dev, python -m http.server). Set false for everything else.
- Package installs, builds, and one-shot commands run in FOREGROUND with streaming output.
- Commands have a 5-minute wall-clock timeout to prevent indefinite hangs.
- To poll: call `check_background_process` with `"wait_seconds": 15`. Repeat until "exited". Max 10 polls.
- Use `list_background_processes` to see all tracked background processes and their status.

## Research First
- When working with a framework or library you're not fully confident about, search the web (browser_navigate to Google) to find docs, then `browser_get_text` to read them. Your training data may be outdated.
- When you hit a blocker, search the web to investigate — never write Python/curl scripts to fetch web pages.
- Prefer official docs over Stack Overflow or blog posts.

## Sub-Agents
- Use `spawn_agent` for complex sub-tasks that might use lots of context (installing software, large file operations, research).
- The agent runs in isolation with a fresh context and returns a summary of what it did.
- Use it when a sub-task would consume too many tokens in your current context window.
- The agent has access to the same tools as you.

## Notifications
- Use send_telegram to notify the user about important events (task completion, errors requiring attention).

## After calling a tool, the system injects the result automatically. Wait for it before continuing."#.to_string()
}

/// Get a behavioral-only system prompt for Jinja template mode.
///
/// This is a stripped-down version of the universal system prompt that contains
/// only behavioral instructions and environment info. Tool format and tool
/// definitions are NOT included — the Jinja template injects those natively
/// via its `{% if tools %}` block.
pub fn get_behavioral_system_prompt() -> String {
    let (os_name, cwd, shell) = env_block();
    let datetime = current_datetime_string();
    let behavior = core_behavior_block();

    format!(
        r#"You are a helpful AI assistant with full system access.

{behavior}

## Current Environment
- Date: {datetime}
- OS: {os_name}
- Working Directory: {cwd}
- Shell: {shell}
"#
    )
}

/// Try to render a prompt using the model's native Jinja2 chat template.
///
/// Returns Ok(prompt) on success, or Err(reason) to trigger fallback to hardcoded templates.
fn try_jinja_render(
    template_str: &str,
    conversation: &str,
    bos_token: &str,
    eos_token: &str,
    mcp_tools: Option<&[McpToolDef]>,
) -> Result<String, String> {
    let system_prompt = get_behavioral_system_prompt();
    let messages = parse_conversation_for_jinja(conversation, &system_prompt);
    let tools = get_available_tools_openai_with_mcp(mcp_tools);

    apply_native_chat_template(
        template_str,
        messages,
        Some(tools),
        None,
        true,
        bos_token,
        eos_token,
    )
}

/// Get the universal system prompt using model-specific tool tags.
pub fn get_universal_system_prompt_with_tags(tags: &ToolTags) -> String {
    let (os_name, cwd, shell) = env_block();
    let datetime = current_datetime_string();
    let behavior = core_behavior_block();

    // OS-specific command examples
    let list_cmd = if os_name == "windows" { "dir" } else { "ls -la" };

    // Harmony models (gpt-oss-20b) use a completely different tool calling format
    if tags.output_open.contains("<|start|>tool") {
        return get_harmony_system_prompt(&os_name, &cwd, shell, list_cmd);
    }

    let exec_open = &tags.exec_open;
    let exec_close = &tags.exec_close;
    let output_open = &tags.output_open;
    let output_close = &tags.output_close;

    format!(
        r#"You are a helpful AI assistant with full system access.

## Tool Calling Format

To use a tool, output a JSON object inside tool tags:

{exec_open}{{"name": "tool_name", "arguments": {{"param": "value"}}}}{exec_close}

After execution, the system injects the result between {output_open} and {output_close} tags. Do NOT generate {output_open} yourself — wait for the injected result.

## Available Tools

### read_file — Read a file's contents (supports PDF, DOCX, XLSX, PPTX, EPUB, ODT, RTF, CSV, EML, ZIP)
{exec_open}{{"name": "read_file", "arguments": {{"path": "filename.txt"}}}}{exec_close}

### write_file — Write content to a file (creates parent dirs)
{exec_open}{{"name": "write_file", "arguments": {{"path": "output.txt", "content": "Hello world"}}}}{exec_close}

### edit_file — Replace exact text in a file (old_string must appear exactly once)
{exec_open}{{"name": "edit_file", "arguments": {{"path": "file.txt", "old_string": "old text", "new_string": "new text"}}}}{exec_close}

### undo_edit — Revert the last edit_file on a file
{exec_open}{{"name": "undo_edit", "arguments": {{"path": "file.txt"}}}}{exec_close}

### insert_text — Insert text at a specific line number
{exec_open}{{"name": "insert_text", "arguments": {{"path": "file.txt", "line": 5, "text": "new line here"}}}}{exec_close}

### search_files — Search file contents by pattern (regex or literal) across a directory
{exec_open}{{"name": "search_files", "arguments": {{"pattern": "TODO", "path": "src", "include": "*.rs"}}}}{exec_close}

### find_files — Find files by name pattern recursively
{exec_open}{{"name": "find_files", "arguments": {{"pattern": "*.py", "path": "."}}}}{exec_close}

### list_directory — List files in a directory
{exec_open}{{"name": "list_directory", "arguments": {{"path": "."}}}}{exec_close}

### execute_command — Run a shell command (background flag REQUIRED)
{exec_open}{{"name": "execute_command", "arguments": {{"command": "{list_cmd}", "background": false}}}}{exec_close}
For servers/daemons: {exec_open}{{"name": "execute_command", "arguments": {{"command": "php artisan serve", "background": true}}}}{exec_close}

### git_status — Show working tree status
{exec_open}{{"name": "git_status", "arguments": {{}}}}{exec_close}

### git_diff — Show git diff (use staged: true for staged changes)
{exec_open}{{"name": "git_diff", "arguments": {{"staged": false}}}}{exec_close}

### git_commit — Commit staged changes (use all: true to auto-stage tracked files)
{exec_open}{{"name": "git_commit", "arguments": {{"message": "Fix bug in parser"}}}}{exec_close}

### check_background_process — Check a background process by PID
{exec_open}{{"name": "check_background_process", "arguments": {{"pid": 12345, "wait_seconds": 15}}}}{exec_close}

### list_background_processes — List all tracked background processes with status
{exec_open}{{"name": "list_background_processes", "arguments": {{}}}}{exec_close}

### send_telegram — Send a notification to the user via Telegram
{exec_open}{{"name": "send_telegram", "arguments": {{"message": "Task completed successfully!"}}}}{exec_close}

### spawn_agent — Spawn a sub-agent for an isolated sub-task (fresh context)
{exec_open}{{"name": "spawn_agent", "arguments": {{"task": "Install Node.js and set up a React project", "context": "Target directory: E:/projects/myapp"}}}}{exec_close}

### browser_search — Search the web (returns Google results with titles, URLs, snippets)
{exec_open}{{"name": "browser_search", "arguments": {{"query": "rust async tutorial"}}}}{exec_close}

### browser_navigate — Open a specific page
{exec_open}{{"name": "browser_navigate", "arguments": {{"url": "https://example.com"}}}}{exec_close}

### browser_get_text — Read page content (use summary param to save tokens)
{exec_open}{{"name": "browser_get_text", "arguments": {{"summary": "extract the main article text"}}}}{exec_close}

### browser_query — Extract structured data using CSS selectors
{exec_open}{{"name": "browser_query", "arguments": {{"selector": "h2 a", "attributes": "text,href", "limit": 10}}}}{exec_close}

### take_screenshot — Capture the user's screen (use monitor=-1 to list monitors)
{exec_open}{{"name": "take_screenshot", "arguments": {{"monitor": 0}}}}{exec_close}

### click_screen — Click at screen coordinates (auto-screenshots after)
{exec_open}{{"name": "click_screen", "arguments": {{"x": 500, "y": 300}}}}{exec_close}

### type_text — Type text via keyboard input
{exec_open}{{"name": "type_text", "arguments": {{"text": "hello"}}}}{exec_close}

### press_key — Press key or combo (e.g., "ctrl+c", "enter", "alt+tab")
{exec_open}{{"name": "press_key", "arguments": {{"key": "enter"}}}}{exec_close}

## Parallel Tool Calls

To call multiple tools at once, use a JSON array:

{exec_open}[
  {{"name": "browser_navigate", "arguments": {{"url": "https://example.com"}}}},
  {{"name": "browser_get_text", "arguments": {{}}}}
]{exec_close}

Independent tools execute concurrently. Use this for multiple independent lookups or file reads.

{behavior}

## Current Environment
- Date: {datetime}
- OS: {os_name}
- Working Directory: {cwd}
- Shell: {shell}
"#
    )
}

/// Generate a system prompt for Harmony models (gpt-oss-20b).
/// These models use native `to=tool_name code<|message|>{JSON}<|call|>` format.
fn get_harmony_system_prompt(os_name: &str, cwd: &str, shell: &str, list_cmd: &str) -> String {
    let datetime = current_datetime_string();
    let behavior = core_behavior_block();

    format!(
        r#"You are a helpful AI assistant with full system access.

## Available Tools

### execute_command — Run a shell command
to=execute_command code<|message|>{{"command": "{list_cmd}"}}<|call|>

### execute_python — Run Python code
to=execute_python code<|message|>{{"code": "print('hello')"}}<|call|>

### read_file — Read a file's contents (supports PDF, DOCX, XLSX, PPTX, EPUB, ODT, RTF, CSV, EML, ZIP)
to=read_file code<|message|>{{"path": "filename.txt"}}<|call|>

### write_file — Write content to a file (creates parent dirs)
to=write_file code<|message|>{{"path": "output.txt", "content": "Hello world"}}<|call|>

### edit_file — Replace exact text in a file (old_string must appear exactly once)
to=edit_file code<|message|>{{"path": "file.txt", "old_string": "old text", "new_string": "new text"}}<|call|>

### undo_edit — Revert the last edit_file operation
to=undo_edit code<|message|>{{"path": "file.txt"}}<|call|>

### insert_text — Insert text at a specific line number
to=insert_text code<|message|>{{"path": "file.txt", "line": 5, "text": "new line content"}}<|call|>

### search_files — Search file contents by regex or literal pattern
to=search_files code<|message|>{{"pattern": "TODO", "path": "src", "include": "*.rs"}}<|call|>

### find_files — Find files by name pattern recursively
to=find_files code<|message|>{{"pattern": "*.rs", "path": "src"}}<|call|>

### list_directory — List files in a directory
to=list_directory code<|message|>{{"path": "."}}<|call|>

### git_status — Show working tree status
to=git_status code<|message|>{{}}<|call|>

### git_diff — Show git diff (use staged: true for staged changes)
to=git_diff code<|message|>{{"staged": false}}<|call|>

### git_commit — Commit staged changes
to=git_commit code<|message|>{{"message": "Fix bug"}}<|call|>

### check_background_process — Check a background process by PID
to=check_background_process code<|message|>{{"pid": 12345, "wait_seconds": 15}}<|call|>

### list_background_processes — List all tracked background processes
to=list_background_processes code<|message|>{{}}<|call|>

### browser_navigate — Open a page in the browser (visible to user)
to=browser_navigate code<|message|>{{"url": "https://example.com"}}<|call|>

### browser_get_text — Read visible text from the current browser page
to=browser_get_text code<|message|>{{}}<|call|>

### take_screenshot — Capture the user's screen
to=take_screenshot code<|message|>{{"monitor": 0}}<|call|>

### send_telegram — Send a notification to the user via Telegram
to=send_telegram code<|message|>{{"message": "Task completed successfully!"}}<|call|>

### spawn_agent — Spawn a sub-agent for an isolated sub-task (fresh context)
to=spawn_agent code<|message|>{{"task": "Install Node.js and set up a React project"}}<|call|>

{behavior}

## Current Environment
- Date: {datetime}
- OS: {os_name}
- Working Directory: {cwd}
- Shell: {shell}
"#
    )
}

/// Apply system prompt with model-specific tool tags.
///
/// Primary path: render using the model's native Jinja2 chat template.
/// Fallback: hardcoded template branches with tool tags in system prompt.
pub fn apply_system_prompt_by_type_with_tags(
    conversation: &str,
    template_type: Option<&str>,
    chat_template_string: Option<&str>,
    tags: &ToolTags,
    bos_token: &str,
    eos_token: &str,
    mcp_tools: Option<&[McpToolDef]>,
) -> Result<String, String> {
    // Try Jinja template first (primary path), fall back to hardcoded templates
    if let Some(template_str) = chat_template_string {
        sys_info!("Trying Jinja template rendering (primary path, template len={})", template_str.len());
        match try_jinja_render(template_str, conversation, bos_token, eos_token, mcp_tools) {
            Ok(prompt) => {
                sys_info!("Jinja template rendered successfully ({} chars)", prompt.len());
                return Ok(prompt);
            }
            Err(e) => {
                sys_warn!("Jinja render failed ({}), falling back to hardcoded templates", e);
            }
        }
    } else {
        sys_info!("No Jinja template available, using hardcoded path");
    }
    // Fallback: hardcoded template with tool tags in system prompt
    sys_info!("Using hardcoded template (type={:?})", template_type);
    apply_model_chat_template_with_tags(conversation, template_type, tags, mcp_tools)
}

/// Apply chat template formatting to conversation history (uses default tags).
#[cfg(test)]
pub fn apply_model_chat_template(
    conversation: &str,
    template_type: Option<&str>,
) -> Result<String, String> {
    use super::tool_tags;
    apply_model_chat_template_with_tags(conversation, template_type, &tool_tags::default_tags(), None)
}

/// Apply chat template formatting to conversation history.
///
/// Parses conversation text and formats it according to the model's chat template type.
/// Uses model-specific tool tags in the system prompt for better tool-calling compliance.
pub fn apply_model_chat_template_with_tags(
    conversation: &str,
    template_type: Option<&str>,
    tags: &ToolTags,
    mcp_tools: Option<&[McpToolDef]>,
) -> Result<String, String> {
    // Parse conversation into messages
    let mut user_messages = Vec::new();
    let mut assistant_messages = Vec::new();
    let mut current_role = "";
    let mut current_content = String::new();

    for line in conversation.lines() {
        if line.ends_with(":")
            && (line.starts_with("SYSTEM:")
                || line.starts_with("USER:")
                || line.starts_with("ASSISTANT:"))
        {
            // Save previous role's content
            if !current_role.is_empty() && !current_content.trim().is_empty() {
                match current_role {
                    "USER" => user_messages.push(current_content.trim().to_string()),
                    "ASSISTANT" => assistant_messages.push(current_content.trim().to_string()),
                    _ => {}
                }
            }

            // Start new role
            current_role = line.trim_end_matches(":");
            current_content.clear();
        } else if !line.starts_with("[COMMAND:") {
            // Skip old command execution logs, add content
            if !line.trim().is_empty() {
                current_content.push_str(line);
                current_content.push('\n');
            }
        }
    }

    // Add the final role content
    if !current_role.is_empty() && !current_content.trim().is_empty() {
        match current_role {
            "USER" => user_messages.push(current_content.trim().to_string()),
            "ASSISTANT" => assistant_messages.push(current_content.trim().to_string()),
            _ => {}
        }
    }

    // Get the universal system prompt with model-specific tool tags
    let universal_prompt = get_universal_system_prompt_with_tags(tags);

    // Use the model-specific universal prompt directly.
    // The conversation's SYSTEM: block may contain a stale copy with default tags,
    // so we always use the freshly-generated prompt with correct model-specific tags.
    let mut final_system_message = universal_prompt;

    // Append MCP tool documentation for hardcoded template path
    if let Some(mcp) = mcp_tools {
        if !mcp.is_empty() {
            let exec_open = &tags.exec_open;
            let exec_close = &tags.exec_close;
            final_system_message.push_str("\n\n## MCP (External) Tools\n\n");
            final_system_message.push_str("The following tools are provided by external MCP servers:\n\n");
            for tool in mcp {
                final_system_message.push_str(&format!(
                    "### {} — [MCP:{}] {}\n{exec_open}{{\"name\": \"{}\", \"arguments\": <see schema>}}{exec_close}\nParameters: {}\n\n",
                    tool.qualified_name,
                    tool.server_name,
                    tool.description,
                    tool.qualified_name,
                    serde_json::to_string(&tool.input_schema).unwrap_or_else(|_| "{}".to_string()),
                ));
            }
        }
    }

    // Construct prompt based on detected template type
    let prompt = match template_type {
        Some("ChatML") => {
            // Qwen/ChatML format: <|im_start|>role\ncontent<|im_end|>
            let mut p = String::new();

            // System message with universal SYSTEM.EXEC prompt
            p.push_str("<|im_start|>system\n");
            p.push_str(&final_system_message);
            p.push_str("<|im_end|>\n");

            // DEBUG: Print the system prompt
            log_debug!("system", "=== CHATML SYSTEM PROMPT ===");
            log_debug!("system", "{}", &final_system_message);
            log_debug!("system", "=== END SYSTEM PROMPT ===");

            // Add conversation history
            let turn_count = user_messages.len().max(assistant_messages.len());
            for i in 0..turn_count {
                if i < user_messages.len() {
                    p.push_str("<|im_start|>user\n");
                    p.push_str(&user_messages[i]);
                    p.push_str("<|im_end|>\n");
                }
                if i < assistant_messages.len() {
                    p.push_str("<|im_start|>assistant\n");
                    p.push_str(&assistant_messages[i]);
                    p.push_str("<|im_end|>\n");
                }
            }

            // Add generation prompt
            p.push_str("<|im_start|>assistant\n");

            p
        }
        Some("Mistral") | None => {
            // Mistral format: <s>[INST] user [/INST] assistant </s>
            let mut p = String::new();
            p.push_str("<s>");

            // System prompt with universal SYSTEM.EXEC instructions
            p.push_str("[SYSTEM_PROMPT]");
            p.push_str(&final_system_message);
            p.push_str("[/SYSTEM_PROMPT]");

            // OLD: Tool injection commented out
            // let tools_json = get_available_tools_json();
            // p.push_str("[AVAILABLE_TOOLS]");
            // p.push_str(&tools_json);
            // p.push_str("[/AVAILABLE_TOOLS]");

            // Add conversation history
            let turn_count = user_messages.len().max(assistant_messages.len());
            for i in 0..turn_count {
                if i < user_messages.len() {
                    p.push_str("[INST]");
                    p.push_str(&user_messages[i]);
                    p.push_str("[/INST]");
                }
                if i < assistant_messages.len() {
                    p.push_str(&assistant_messages[i]);
                    p.push_str("</s>");
                }
            }

            p
        }
        Some("Llama3") => {
            // Llama 3 format
            let mut p = String::new();
            p.push_str("<|begin_of_text|>");

            // System message with universal SYSTEM.EXEC prompt
            p.push_str("<|start_header_id|>system<|end_header_id|>\n\n");
            p.push_str(&final_system_message);
            p.push_str("<|eot_id|>");

            let turn_count = user_messages.len().max(assistant_messages.len());
            for i in 0..turn_count {
                if i < user_messages.len() {
                    p.push_str("<|start_header_id|>user<|end_header_id|>\n\n");
                    p.push_str(&user_messages[i]);
                    p.push_str("<|eot_id|>");
                }
                if i < assistant_messages.len() {
                    p.push_str("<|start_header_id|>assistant<|end_header_id|>\n\n");
                    p.push_str(&assistant_messages[i]);
                    p.push_str("<|eot_id|>");
                }
            }

            p.push_str("<|start_header_id|>assistant<|end_header_id|>\n\n");

            p
        }
        Some("Gemma") => {
            // Gemma 3 format: <start_of_turn>role\ncontent<end_of_turn>\n
            // Note: Gemma uses "model" instead of "assistant"
            let mut p = String::new();

            // Gemma doesn't have a system role, so prepend to first user message
            let first_user_prefix = format!("{final_system_message}\n\n");

            // Add conversation history
            let turn_count = user_messages.len().max(assistant_messages.len());
            for i in 0..turn_count {
                if i < user_messages.len() {
                    p.push_str("<start_of_turn>user\n");
                    // Add system prompt prefix to first user message
                    if i == 0 {
                        p.push_str(&first_user_prefix);
                    }
                    p.push_str(&user_messages[i]);
                    p.push_str("<end_of_turn>\n");
                }
                if i < assistant_messages.len() {
                    p.push_str("<start_of_turn>model\n");
                    p.push_str(&assistant_messages[i]);
                    p.push_str("<end_of_turn>\n");
                }
            }

            // Add generation prompt
            p.push_str("<start_of_turn>model\n");

            p
        }
        Some("Phi") => {
            // Phi-3/Phi-4 format: <|system|>content<|end|>\n<|user|>content<|end|>\n<|assistant|>
            let mut p = String::new();

            // System message
            p.push_str("<|system|>\n");
            p.push_str(&final_system_message);
            p.push_str("<|end|>\n");

            // Add conversation history
            let turn_count = user_messages.len().max(assistant_messages.len());
            for i in 0..turn_count {
                if i < user_messages.len() {
                    p.push_str("<|user|>\n");
                    p.push_str(&user_messages[i]);
                    p.push_str("<|end|>\n");
                }
                if i < assistant_messages.len() {
                    p.push_str("<|assistant|>\n");
                    p.push_str(&assistant_messages[i]);
                    p.push_str("<|end|>\n");
                }
            }

            // Add generation prompt
            p.push_str("<|assistant|>\n");

            p
        }
        Some("GLM") => {
            // GLM-4 family: [gMASK]<sop><|system|>content<|user|>\ncontent<|assistant|>\n
            let mut p = String::new();

            // System message
            p.push_str("[gMASK]<sop><|system|>\n");
            p.push_str(&final_system_message);

            // Add conversation history
            let turn_count = user_messages.len().max(assistant_messages.len());
            for i in 0..turn_count {
                if i < user_messages.len() {
                    p.push_str("<|user|>\n");
                    p.push_str(&user_messages[i]);
                }
                if i < assistant_messages.len() {
                    p.push_str("<|assistant|>\n");
                    p.push_str(&assistant_messages[i]);
                }
            }

            // Add generation prompt
            p.push_str("<|assistant|>\n");

            p
        }
        Some(_) => {
            // Generic fallback
            let mut p = String::new();

            p.push_str("System: ");
            p.push_str(&final_system_message);
            p.push_str("\n\n");

            let turn_count = user_messages.len().max(assistant_messages.len());
            for i in 0..turn_count {
                if i < user_messages.len() {
                    p.push_str("User: ");
                    p.push_str(&user_messages[i]);
                    p.push_str("\n\n");
                }
                if i < assistant_messages.len() {
                    p.push_str("Assistant: ");
                    p.push_str(&assistant_messages[i]);
                    p.push_str("\n\n");
                }
            }

            p.push_str("Assistant: ");

            p
        }
    };

    // Debug: Print first 1000 chars of prompt
    log_debug!("system", "\nTemplate type: {:?}", template_type);
    log_debug!("system", "Constructed prompt (first 1000 chars):");
    log_debug!(
        "system",
        "{}",
        &prompt.chars().take(1000).collect::<String>()
    );

    Ok(prompt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_universal_system_prompt_contains_exec_tags() {
        use crate::web::chat::tool_tags;
        let prompt = get_universal_system_prompt_with_tags(&tool_tags::default_tags());
        assert!(prompt.contains("<||SYSTEM.EXEC>"));
        assert!(prompt.contains("<SYSTEM.EXEC||>"));
        assert!(prompt.contains("<||SYSTEM.OUTPUT>"));
        assert!(prompt.contains("<SYSTEM.OUTPUT||>"));
    }

    #[test]
    fn test_universal_system_prompt_contains_os_info() {
        use crate::web::chat::tool_tags;
        let prompt = get_universal_system_prompt_with_tags(&tool_tags::default_tags());
        // Should contain OS info
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

        // Should not error, should handle empty content gracefully
        assert!(result.is_ok());
    }

    #[test]
    fn test_template_includes_universal_prompt() {
        let conversation = "USER:\nTest message";
        let result = apply_model_chat_template(conversation, Some("ChatML")).unwrap();

        // Should include the universal SYSTEM.EXEC tags
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
        use crate::web::chat::tool_tags;

        // Qwen tags
        let qwen_tags = tool_tags::get_tool_tags_for_model(Some("Qwen3 8B"));
        let prompt = get_universal_system_prompt_with_tags(&qwen_tags);
        assert!(prompt.contains("<tool_call>"), "Qwen prompt should use <tool_call> tags");
        assert!(prompt.contains("</tool_call>"), "Qwen prompt should use </tool_call> tags");
        assert!(!prompt.contains("SYSTEM.EXEC"), "Qwen prompt should NOT contain SYSTEM.EXEC");

        // Mistral tags
        let mistral_tags = tool_tags::get_tool_tags_for_model(Some("mistralai_Devstral Small 2507"));
        let prompt = get_universal_system_prompt_with_tags(&mistral_tags);
        assert!(prompt.contains("[TOOL_CALLS]"), "Mistral prompt should use [TOOL_CALLS] tags");
        assert!(prompt.contains("[/TOOL_CALLS]"), "Mistral prompt should use [/TOOL_CALLS] tags");

        // Unknown model (default tags)
        let default_tags = tool_tags::get_tool_tags_for_model(Some("SomeUnknownModel"));
        let prompt = get_universal_system_prompt_with_tags(&default_tags);
        assert!(prompt.contains("<||SYSTEM.EXEC>"), "Unknown model should use default SYSTEM.EXEC tags");
    }
}
