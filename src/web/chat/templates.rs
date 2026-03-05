use crate::{log_debug, sys_info, sys_warn};
use super::jinja_templates::{
    apply_native_chat_template, get_available_tools_openai, parse_conversation_for_jinja,
};
use super::tool_tags::ToolTags;

/// Get a behavioral-only system prompt for Jinja template mode.
///
/// This is a stripped-down version of the universal system prompt that contains
/// only behavioral instructions and environment info. Tool format and tool
/// definitions are NOT included — the Jinja template injects those natively
/// via its `{% if tools %}` block.
pub fn get_behavioral_system_prompt() -> String {
    let os_name = std::env::consts::OS;
    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| ".".to_string());

    let shell = if os_name == "windows" {
        "cmd/powershell"
    } else {
        "bash"
    };

    format!(
        r#"You are a helpful AI assistant with full system access.

## Behavior
- Be autonomous and resourceful. Complete tasks fully without asking the user for help.
- If a command fails, try a DIFFERENT alternative approach. Do NOT retry the same failing command.
- If a tool is not in PATH, use its full path (e.g., `C:\php\php.exe` instead of `php`), or download it to a known location and reference it by full path.
- After downloading a tool, use its full path to run it. Do NOT assume it is in PATH.
- Do NOT tell the user to run commands manually — use your tools to solve problems yourself.
- NEVER repeat the same failing command more than once. If it failed, change your approach.
- When creating files, use `write_file` to create them directly. Do not just show the code and ask the user to copy it.
- When a task requires multiple steps, execute them one by one using your tools. Do not skip steps.
- For any command that might take over 30 seconds (package installs like `composer create-project`, `npm install`, builds, downloads), use `execute_command` with `"background": true`. This returns after 5s with the PID. Then use `check_background_process` with the PID to poll for completion — you can do other work in between.

## Important Notes
- Use `read_file` instead of cat/type to read files
- Use `write_file` instead of echo/python to write files
- Use `execute_python` for any Python code (avoids shell quoting issues)
- Use `execute_command` for shell tools like git, npm, curl, etc.
- For long-running commands (dev servers, watchers, package installs, builds), use `execute_command` with `"background": true` — this returns after 5s with initial output and the PID.
- Use `check_background_process` with the PID to poll progress. When it reports "exited", the command is done.
- Use `web_search` to find information online, then `web_fetch` to read specific pages
- After calling a tool, the system will inject the result automatically. Wait for it before continuing.

## Research First
- When working with a framework or library you are not 100% confident about (Laravel, Next.js, Django, etc.), use `web_fetch` to read the official documentation BEFORE writing code. Your training data may be outdated.
- This also applies to APIs, CLI tools, or any technology where the exact syntax or project structure may have changed.
- Prefer official docs over Stack Overflow or blog posts.

## Current Environment
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
) -> Result<String, String> {
    let system_prompt = get_behavioral_system_prompt();
    let messages = parse_conversation_for_jinja(conversation, &system_prompt);
    let tools = get_available_tools_openai();

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
    let os_name = std::env::consts::OS;
    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| ".".to_string());

    let shell = if os_name == "windows" {
        "cmd/powershell"
    } else {
        "bash"
    };

    // OS-specific command examples
    let list_cmd = if os_name == "windows" { "dir" } else { "ls -la" };

    // Harmony models (gpt-oss-20b) use a completely different tool calling format
    if tags.output_open.contains("<|start|>tool") {
        return get_harmony_system_prompt(os_name, &cwd, shell, list_cmd);
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

After execution, the system will inject the result between {output_open} and {output_close} tags. Do NOT generate {output_open} yourself — the system does this automatically. Wait for the injected result before continuing.

## Available Tools

### read_file — Read a file's contents (supports PDF text extraction)
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

### git_status — Show working tree status (modified, staged, untracked files)
{exec_open}{{"name": "git_status", "arguments": {{}}}}{exec_close}

### git_diff — Show git diff (unstaged by default, use staged: true for staged changes)
{exec_open}{{"name": "git_diff", "arguments": {{"staged": false}}}}{exec_close}

### git_commit — Commit staged changes (use all: true to auto-stage tracked modified files)
{exec_open}{{"name": "git_commit", "arguments": {{"message": "Fix bug in parser"}}}}{exec_close}

### execute_python — Run Python code (multi-line, imports, regex all work)
{exec_open}{{"name": "execute_python", "arguments": {{"code": "import json\ndata = {{'key': 'value'}}\nprint(json.dumps(data, indent=2))"}}}}{exec_close}

### execute_command — Run a shell command. Add "background": true for long commands (installs, builds, servers).
{exec_open}{{"name": "execute_command", "arguments": {{"command": "{list_cmd}"}}}}{exec_close}
Background example (returns after 5s with PID): {exec_open}{{"name": "execute_command", "arguments": {{"command": "composer create-project laravel/laravel myapp", "background": true}}}}{exec_close}

### check_background_process — Poll a background process by PID (new output + running/exited status)
{exec_open}{{"name": "check_background_process", "arguments": {{"pid": 12345}}}}{exec_close}

### list_directory — List files in a directory
{exec_open}{{"name": "list_directory", "arguments": {{"path": "."}}}}{exec_close}

### web_search — Search the web using the configured provider
{exec_open}{{"name": "web_search", "arguments": {{"query": "rust async tutorial"}}}}{exec_close}

### web_fetch — Fetch a web page and return its text content
{exec_open}{{"name": "web_fetch", "arguments": {{"url": "https://example.com"}}}}{exec_close}

## Parallel Tool Calls

To call multiple tools at once, use a JSON array inside the tool tags:

{exec_open}[
  {{"name": "web_search", "arguments": {{"query": "latest news topic A"}}}},
  {{"name": "web_search", "arguments": {{"query": "latest news topic B"}}}}
]{exec_close}

Independent tools in the array execute concurrently for faster results. Their outputs are returned together in a single {output_open}...{output_close} block. Use this when you need multiple independent pieces of information at the same time (e.g., searching for multiple topics, reading multiple files).

## Behavior
- Be autonomous and resourceful. Complete tasks fully without asking the user for help.
- If a command fails, try a DIFFERENT alternative approach. Do NOT retry the same failing command.
- If a tool is not in PATH, use its full path (e.g., `C:\php\php.exe` instead of `php`), or download it to a known location and reference it by full path.
- After downloading a tool, use its full path to run it. Do NOT assume it is in PATH.
- Do NOT tell the user to run commands manually — use your tools to solve problems yourself.
- NEVER repeat the same failing command more than once. If it failed, change your approach.
- When creating files, use `write_file` to create them directly. Do not just show the code and ask the user to copy it.
- Use `edit_file` for small changes to existing files instead of rewriting them entirely with `write_file`.
- When a task requires multiple steps, execute them one by one using your tools. Do not skip steps.
- For any command that might take over 30 seconds (package installs like `composer create-project`, `npm install`, builds, downloads), use `execute_command` with `"background": true`. This returns after 5s with the PID. Then use `check_background_process` to poll for completion.

## Important Notes
- Use `read_file` instead of cat/type to read files
- Use `write_file` to create new files; use `edit_file` to modify existing files
- Use `search_files` instead of grep/findstr to search file contents
- Use `find_files` instead of find/dir to locate files by name pattern
- Use `insert_text` to add lines at a specific position (e.g. imports, new functions)
- Use `undo_edit` if an edit_file went wrong — restores the previous version
- Use `git_status`, `git_diff`, `git_commit` for git operations instead of `execute_command`
- Use `execute_python` for any Python code (avoids shell quoting issues)
- Use `execute_command` for shell tools like npm, curl, etc. Use `"background": true` for long commands (installs, builds, servers).
- Use `check_background_process` to poll background processes. When it reports "exited", the command is done.
- Use `web_search` to find information online, then `web_fetch` to read specific pages
- You can also put raw shell commands directly: {exec_open}{list_cmd}{exec_close}

## Current Environment
- OS: {os_name}
- Working Directory: {cwd}
- Shell: {shell}
"#
    )
}

/// Generate a system prompt for Harmony models (gpt-oss-20b).
/// These models use native `to=tool_name code<|message|>{JSON}<|call|>` format.
fn get_harmony_system_prompt(os_name: &str, cwd: &str, shell: &str, list_cmd: &str) -> String {
    format!(
        r#"You are a helpful AI assistant with full system access. You can execute tools to help the user.

## Available Tools

You have these tools available. To call a tool, use the Harmony tool call format:

### execute_command — Run a shell command. Add "background": true for long commands (installs, builds, servers).
to=execute_command code<|message|>{{"command": "{list_cmd}"}}<|call|>
Background example (returns after 5s with PID): to=execute_command code<|message|>{{"command": "composer create-project laravel/laravel myapp", "background": true}}<|call|>

### check_background_process — Poll a background process by PID (new output + running/exited status)
to=check_background_process code<|message|>{{"pid": 12345}}<|call|>

### list_directory — List files in a directory
to=list_directory code<|message|>{{"path": "."}}<|call|>

### read_file — Read a file's contents (supports PDF text extraction)
to=read_file code<|message|>{{"path": "filename.txt"}}<|call|>

### write_file — Write content to a file (creates parent dirs)
to=write_file code<|message|>{{"path": "output.txt", "content": "Hello world"}}<|call|>

### edit_file — Replace exact text in a file (old_string must appear exactly once)
to=edit_file code<|message|>{{"path": "file.txt", "old_string": "old text", "new_string": "new text"}}<|call|>

### undo_edit — Revert the last edit_file operation on a file
to=undo_edit code<|message|>{{"path": "file.txt"}}<|call|>

### insert_text — Insert text at a specific line number in a file (1-based)
to=insert_text code<|message|>{{"path": "file.txt", "line": 5, "text": "new line content"}}<|call|>

### search_files — Search file contents by regex or literal pattern
to=search_files code<|message|>{{"pattern": "TODO", "path": "src", "include": "*.rs"}}<|call|>

### find_files — Find files by name pattern recursively
to=find_files code<|message|>{{"pattern": "*.rs", "path": "src"}}<|call|>

### git_status — Show working tree status (modified, staged, untracked files)
to=git_status code<|message|>{{}}<|call|>

### git_diff — Show git diff (unstaged by default, use staged: true for staged)
to=git_diff code<|message|>{{"staged": false}}<|call|>

### git_commit — Commit staged changes
to=git_commit code<|message|>{{"message": "Fix bug"}}<|call|>

### execute_python — Run Python code (multi-line, imports, regex all work)
to=execute_python code<|message|>{{"code": "print('hello')"}}<|call|>

### web_search — Search the web using the configured provider
to=web_search code<|message|>{{"query": "rust async tutorial"}}<|call|>

### web_fetch — Fetch a web page and return its text content
to=web_fetch code<|message|>{{"url": "https://example.com"}}<|call|>

## Behavior
- Be autonomous and resourceful. Complete tasks fully without asking the user for help.
- If a command fails, try a DIFFERENT alternative approach. Do NOT retry the same failing command.
- If a tool is not in PATH, use its full path (e.g., `C:\php\php.exe` instead of `php`), or download it to a known location and reference it by full path.
- After downloading a tool, use its full path to run it. Do NOT assume it is in PATH.
- Do NOT tell the user to run commands manually — use your tools to solve problems yourself.
- NEVER repeat the same failing command more than once. If it failed, change your approach.
- When creating files, use `write_file` to create them directly. Do not just show the code and ask the user to copy it.
- When a task requires multiple steps, execute them one by one using your tools. Do not skip steps.
- For any command that might take over 30 seconds (package installs like `composer create-project`, `npm install`, builds, downloads), use `execute_command` with `"background": true`. Then use `check_background_process` to poll for completion.

## Important Notes
- Always use these tools when the user asks you to interact with the filesystem or run commands.
- After you call a tool, the system will inject the result automatically. Wait for it before continuing.
- Use `read_file` instead of cat/type to read files.
- Use `edit_file` for small changes. Use `write_file` only for new files or full rewrites.
- Use `insert_text` to add new lines (imports, functions) without needing surrounding context.
- Use `undo_edit` to revert a bad edit_file operation.
- Use `search_files` to find patterns across code. Use `find_files` to locate files by name.
- Use `git_status`, `git_diff`, `git_commit` for git operations instead of `execute_command`.
- Use `execute_command` for shell tools like npm, curl, etc. Use `"background": true` for long commands (installs, builds, servers).
- Use `check_background_process` to poll background processes. When it reports "exited", the command is done.
- Use `web_search` to find information online, then `web_fetch` to read specific pages.

## Current Environment
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
) -> Result<String, String> {
    // Try Jinja template first (primary path), fall back to hardcoded templates
    if let Some(template_str) = chat_template_string {
        sys_info!("Trying Jinja template rendering (primary path, template len={})", template_str.len());
        match try_jinja_render(template_str, conversation, bos_token, eos_token) {
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
    apply_model_chat_template_with_tags(conversation, template_type, tags)
}

/// Apply chat template formatting to conversation history (uses default tags).
#[cfg(test)]
pub fn apply_model_chat_template(
    conversation: &str,
    template_type: Option<&str>,
) -> Result<String, String> {
    use super::tool_tags;
    apply_model_chat_template_with_tags(conversation, template_type, &tool_tags::default_tags())
}

/// Apply chat template formatting to conversation history.
///
/// Parses conversation text and formats it according to the model's chat template type.
/// Uses model-specific tool tags in the system prompt for better tool-calling compliance.
pub fn apply_model_chat_template_with_tags(
    conversation: &str,
    template_type: Option<&str>,
    tags: &ToolTags,
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
    let final_system_message = universal_prompt;

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
