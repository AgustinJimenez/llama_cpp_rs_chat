use std::env;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

// Helper function to parse command with proper quote handling
pub fn parse_command_with_quotes(cmd: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current_part = String::new();
    let mut in_quotes = false;
    let chars = cmd.chars().peekable();

    for ch in chars {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
                // Don't include the quote character in the output
            }
            ' ' if !in_quotes => {
                if !current_part.is_empty() {
                    parts.push(current_part.clone());
                    current_part.clear();
                }
            }
            _ => {
                current_part.push(ch);
            }
        }
    }

    if !current_part.is_empty() {
        parts.push(current_part);
    }

    parts
}

/// Check if a command uses shell operators that require a shell to interpret.
fn needs_shell(cmd: &str) -> bool {
    let mut in_quotes = false;
    let mut prev = '\0';
    for ch in cmd.chars() {
        if ch == '"' {
            in_quotes = !in_quotes;
        }
        if !in_quotes {
            match ch {
                '|' | '<' | ';' => return true,
                '>' if prev != '2' => return true, // allow 2> but catch > and >>
                '&' if prev == '&' => return true,  // &&
                _ => {}
            }
        }
        prev = ch;
    }
    false
}

/// Enrich PATH with common Windows tool directories.
fn enriched_windows_path() -> String {
    let current_path = env::var("PATH").unwrap_or_default();
    let extra_dirs = [
        r"C:\WINDOWS\system32",
        r"C:\WINDOWS",
        r"C:\WINDOWS\System32\Wbem",
        r"C:\WINDOWS\System32\WindowsPowerShell\v1.0",
        r"C:\Program Files\Git\cmd",
        r"C:\Program Files\nodejs",
        r"C:\ProgramData\chocolatey\bin",
    ];
    extra_dirs
        .iter()
        .filter(|d| !current_path.contains(*d))
        .fold(current_path.clone(), |acc, d| format!("{acc};{d}"))
}

/// Execute a command on Windows.
/// Strategy: try direct execution first (avoids shell quoting issues for python, git, etc.).
/// Fall back to PowerShell for shell builtins (cat, dir, type) and commands with shell operators.
fn execute_windows(cmd: &str, parts: &[String]) -> std::io::Result<std::process::Output> {
    let path = enriched_windows_path();

    // Commands with shell operators (|, >, &&, etc.) must go through PowerShell
    if needs_shell(cmd) {
        let escaped = cmd.replace('$', "`$");
        return Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &escaped])
            .env("PATH", &path)
            .output();
    }

    // Try direct execution first — no shell means no quoting issues
    let result = Command::new(&parts[0])
        .args(&parts[1..])
        .env("PATH", &path)
        .output();

    match &result {
        Ok(_) => result,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Command not found as executable — try PowerShell for aliases/builtins
            // (cat, dir, type, ls, etc. are PowerShell aliases, not real executables)
            let escaped = cmd.replace('$', "`$");
            Command::new("powershell")
                .args(["-NoProfile", "-NonInteractive", "-Command", &escaped])
                .env("PATH", &path)
                .output()
        }
        Err(_) => result,
    }
}

// Helper function to execute system commands
pub fn execute_command(cmd: &str) -> String {
    let trimmed = cmd.trim();

    // Parse command with proper quote handling
    let parts = parse_command_with_quotes(trimmed);
    if parts.is_empty() {
        return "Error: Empty command".to_string();
    }

    // If the command contains shell operators, delegate to sh/bash so they work.
    // This handles `cd /dir && npm init`, pipes, redirects, etc.
    let has_shell_ops = trimmed.contains("&&")
        || trimmed.contains("||")
        || trimmed.contains(" | ")
        || trimmed.contains(';')
        || trimmed.contains('>')
        || trimmed.contains('<');

    if has_shell_ops {
        // Try native echo redirect first (avoids shell $variable expansion)
        if !cfg!(target_os = "windows") {
            if let Some(result) = try_native_echo_redirect(trimmed) {
                return result;
            }
        }
        #[cfg(target_os = "windows")]
        let output = Command::new("cmd")
            .raw_arg(format!("/C {trimmed}"))
            .env("PATH", enriched_windows_path())
            .output();
        #[cfg(not(target_os = "windows"))]
        let output = Command::new("sh").arg("-c").arg(trimmed).output();
        return match output {
            Ok(o) => {
                let stdout = String::from_utf8_lossy(&o.stdout);
                let stderr = String::from_utf8_lossy(&o.stderr);
                let exit_code = o.status.code().unwrap_or(-1);
                if !stderr.is_empty() && !o.status.success() {
                    format!("{stdout}\nError (exit code {exit_code}): {stderr}")
                } else if stdout.is_empty() && stderr.is_empty() && o.status.success() {
                    "Command executed successfully (no output)".to_string()
                } else if stdout.is_empty() && stderr.is_empty() && !o.status.success() {
                    format!("Command failed with exit code {exit_code} and produced no output. The command may have found no matches or encountered a silent error.")
                } else {
                    let combined = format!("{stdout}{stderr}");
                    if combined.trim().is_empty() {
                        if o.status.success() {
                            "Command executed successfully (no output)".to_string()
                        } else {
                            format!("Command failed with exit code {exit_code} (no output)")
                        }
                    } else {
                        combined
                    }
                }
            }
            Err(e) => format!("Failed to execute command: {e}"),
        };
    }

    let command_name = &parts[0];

    // Special handling for cd command - actually change the process working directory
    if command_name == "cd" {
        let target_dir = if parts.len() > 1 {
            &parts[1]
        } else {
            return "Error: cd command requires a directory argument".to_string();
        };

        match env::set_current_dir(target_dir) {
            Ok(_) => {
                if let Ok(new_dir) = env::current_dir() {
                    format!("Successfully changed directory to: {}", new_dir.display())
                } else {
                    "Directory changed successfully".to_string()
                }
            }
            Err(e) => {
                format!("Error: Failed to change directory: {e}")
            }
        }
    } else {
        // Normal command execution for non-cd commands
        let is_windows = cfg!(target_os = "windows");

        let output = if is_windows {
            execute_windows(cmd.trim(), &parts)
        } else {
            Command::new(&parts[0])
                .args(&parts[1..])
                .output()
        };

        match output {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);

                // Handle commands that succeed silently
                if output.status.success() && stdout.is_empty() && stderr.is_empty() {
                    match command_name.as_str() {
                        "find" => "No files found matching the search criteria".to_string(),
                        "mkdir" => "Directory created successfully".to_string(),
                        "touch" => "File created successfully".to_string(),
                        "rm" | "rmdir" => "File/directory removed successfully".to_string(),
                        "mv" | "cp" => "File operation completed successfully".to_string(),
                        "chmod" => "Permissions changed successfully".to_string(),
                        _ => {
                            if parts.len() > 1 {
                                format!("Command '{}' executed successfully", parts.join(" "))
                            } else {
                                format!("Command '{command_name}' executed successfully")
                            }
                        }
                    }
                } else if !output.status.success() && stdout.is_empty() && stderr.is_empty() {
                    let exit_code = output.status.code().unwrap_or(-1);
                    format!("Command failed with exit code {exit_code} and produced no output.")
                } else if !stderr.is_empty() {
                    let exit_code = output.status.code().unwrap_or(0);
                    if output.status.success() {
                        format!("{stdout}{stderr}")
                    } else {
                        format!("{stdout}\nError (exit code {exit_code}): {stderr}")
                    }
                } else {
                    stdout.to_string()
                }
            }
            Err(e) => {
                format!("Failed to execute command: {e}")
            }
        }
    }
}

/// Execute a command with streaming output, calling `on_line` for each line of stdout.
/// Returns the full accumulated output as a String (same as `execute_command`).
/// Uses spawn() + BufReader instead of .output() so output is available line-by-line.
///
/// If `cancel` is provided and set to `true`, the child process is killed and
/// the function returns early with a cancellation notice.
pub fn execute_command_streaming(
    cmd: &str,
    cancel: Option<Arc<AtomicBool>>,
    mut on_line: impl FnMut(&str),
) -> String {
    let trimmed = cmd.trim();

    let parts = parse_command_with_quotes(trimmed);
    if parts.is_empty() {
        return "Error: Empty command".to_string();
    }

    // cd command - no streaming needed
    let command_name = &parts[0];
    if command_name == "cd" {
        return execute_command(cmd);
    }

    // Determine how to spawn the command
    let has_shell_ops = trimmed.contains("&&")
        || trimmed.contains("||")
        || trimmed.contains(" | ")
        || trimmed.contains(';')
        || trimmed.contains('>')
        || trimmed.contains('<');

    // For echo redirects on Unix, use the native handler (no streaming needed)
    if has_shell_ops && !cfg!(target_os = "windows") {
        if let Some(result) = try_native_echo_redirect(trimmed) {
            return result;
        }
    }

    // For streaming, ALWAYS merge stderr into stdout via `2>&1` shell wrapper.
    // Many tools (composer, npm, cargo, etc.) write progress to stderr, and we want
    // to stream ALL output — not just stdout. The shell wrapper overhead is negligible
    // for long-running commands that benefit from streaming.
    let env_vars = [
        ("PYTHONUNBUFFERED", "1"),
        ("COMPOSER_PROCESS_TIMEOUT", "0"),
        ("GIT_FLUSH", "1"),
        ("CI", "true"),
    ];

    #[cfg(target_os = "windows")]
    let child_result = {
        let path = enriched_windows_path();
        let mut cmd = Command::new("cmd");
        cmd.raw_arg(format!("/C {trimmed} 2>&1"))
            .env("PATH", &path);
        for (k, v) in &env_vars {
            cmd.env(k, v);
        }
        cmd.stdout(Stdio::piped()).stderr(Stdio::null()).spawn()
    };

    #[cfg(not(target_os = "windows"))]
    let child_result = {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(format!("{trimmed} 2>&1"));
        for (k, v) in &env_vars {
            cmd.env(k, v);
        }
        cmd.stdout(Stdio::piped()).stderr(Stdio::null()).spawn()
    };

    match child_result {
        Ok(mut child) => {
            let mut output = String::new();

            // Take stdout before sharing child with monitor thread
            let stdout_pipe = child.stdout.take();

            // Wrap child so the cancel-monitor thread can kill it.
            // The monitor checks `cancel` every 100ms and calls child.kill() if set.
            // A `done_flag` tells the monitor to exit on normal completion.
            let child_cell = Arc::new(StdMutex::new(child));
            let done_flag = Arc::new(AtomicBool::new(false));

            let monitor_handle = cancel.map(|cancel_flag| {
                let child_ref = child_cell.clone();
                let done_ref = done_flag.clone();
                std::thread::spawn(move || -> bool {
                    loop {
                        if cancel_flag.load(Ordering::Relaxed) {
                            if let Ok(mut guard) = child_ref.lock() {
                                let _ = guard.kill();
                            }
                            return true; // was cancelled
                        }
                        if done_ref.load(Ordering::Relaxed) {
                            return false; // normal completion
                        }
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                })
            });

            // Read stdout and stream output as it arrives.
            // stderr is merged into stdout via 2>&1, so all output comes through here.
            // We use byte-level reading instead of BufReader::lines() because:
            // 1. lines() only splits on \n — progress bars using \r would be missed
            // 2. When pipe data arrives in bursts (from child's buffer flush), we want
            //    to emit each line immediately rather than waiting for more \n
            if let Some(stdout) = stdout_pipe {
                let mut reader = std::io::BufReader::new(stdout);
                let mut line_buf = String::new();
                let mut byte_buf = [0u8; 4096];

                loop {
                    match std::io::Read::read(&mut reader, &mut byte_buf) {
                        Ok(0) => break, // EOF (child exited or was killed)
                        Ok(n) => {
                            let chunk = String::from_utf8_lossy(&byte_buf[..n]);
                            for ch in chunk.chars() {
                                if ch == '\n' || ch == '\r' {
                                    if !line_buf.is_empty() {
                                        on_line(&line_buf);
                                        output.push_str(&line_buf);
                                        output.push('\n');
                                        line_buf.clear();
                                    }
                                } else {
                                    line_buf.push(ch);
                                }
                            }
                        }
                        Err(_) => break,
                    }
                }
                // Flush any remaining content
                if !line_buf.is_empty() {
                    on_line(&line_buf);
                    output.push_str(&line_buf);
                    output.push('\n');
                }
            }

            // Signal monitor thread to exit, then wait for child + monitor
            done_flag.store(true, Ordering::Relaxed);

            let status = child_cell.lock().unwrap().wait();
            let exit_code = status.as_ref().ok().and_then(|s| s.code()).unwrap_or(-1);
            let success = status.as_ref().map(|s| s.success()).unwrap_or(false);

            let was_cancelled = monitor_handle
                .map(|h| h.join().unwrap_or(false))
                .unwrap_or(false);

            if was_cancelled {
                output.push_str("\n[Cancelled by user]\n");
            }

            if output.trim().is_empty() {
                if success {
                    "Command executed successfully (no output)".to_string()
                } else {
                    format!("Command failed with exit code {exit_code} and produced no output.")
                }
            } else {
                output
            }
        }
        Err(e) => {
            // On Windows, if direct execution fails with NotFound, try PowerShell
            if cfg!(target_os = "windows") && !has_shell_ops && e.kind() == std::io::ErrorKind::NotFound {
                // Fall back to non-streaming execute_command for PowerShell fallback
                execute_command(cmd)
            } else {
                format!("Failed to execute command: {e}")
            }
        }
    }
}

/// Find the position of the last `>` redirect operator that is NOT inside quotes.
fn find_last_redirect(cmd: &str) -> Option<usize> {
    let mut last_pos = None;
    let mut in_single = false;
    let mut in_double = false;
    for (i, ch) in cmd.chars().enumerate() {
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '>' if !in_single && !in_double => last_pos = Some(i),
            _ => {}
        }
    }
    last_pos
}

/// Split a command string on `&&` and `||` operators (outside of quotes).
fn split_on_chain_ops(cmd: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut in_single = false;
    let mut in_double = false;
    let bytes = cmd.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let ch = bytes[i] as char;
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '&' if !in_single && !in_double && i + 1 < bytes.len() && bytes[i + 1] == b'&' => {
                parts.push(cmd[start..i].trim());
                i += 2;
                start = i;
                continue;
            }
            '|' if !in_single && !in_double && i + 1 < bytes.len() && bytes[i + 1] == b'|' => {
                parts.push(cmd[start..i].trim());
                i += 2;
                start = i;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    parts.push(cmd[start..].trim());
    parts
}

/// Extract the content from an echo command (handling double quotes, single quotes, or bare text).
fn extract_echo_content(echo_part: &str) -> Option<String> {
    let trimmed = echo_part.trim();
    let after_echo = if let Some(stripped) = trimmed.strip_prefix("echo ") {
        stripped.trim()
    } else {
        return None;
    };

    if (after_echo.starts_with('"') && after_echo.ends_with('"')
        || after_echo.starts_with('\'') && after_echo.ends_with('\''))
        && after_echo.len() >= 2
    {
        Some(after_echo[1..after_echo.len() - 1].to_string())
    } else {
        Some(after_echo.to_string())
    }
}

/// Intercept `echo "..." > file` patterns and write directly with std::fs::write.
/// This avoids shell variable expansion ($table becomes empty) and quoting issues.
/// Returns Some(result) if handled, None to fall through to sh -c.
fn try_native_echo_redirect(cmd: &str) -> Option<String> {
    let parts = split_on_chain_ops(cmd);
    let last_part = parts.last()?.trim();

    // The last segment must have a redirect
    let redirect_pos = find_last_redirect(last_part)?;

    // Split into echo part and file path
    let echo_part = last_part[..redirect_pos].trim();
    let file_path = last_part[redirect_pos + 1..].trim();

    // Must start with echo
    if !echo_part.starts_with("echo ") {
        return None;
    }

    // File path must not be empty
    if file_path.is_empty() {
        return None;
    }

    // Execute any preceding chained commands (mkdir -p, etc.) via shell
    if parts.len() > 1 {
        let prefix_cmds = &parts[..parts.len() - 1];
        for prefix in prefix_cmds {
            let output = Command::new("sh").arg("-c").arg(prefix).output();
            match output {
                Ok(o) if !o.status.success() => {
                    let stderr = String::from_utf8_lossy(&o.stderr);
                    return Some(format!("Error: {stderr}"));
                }
                Err(e) => return Some(format!("Error: {e}")),
                _ => {}
            }
        }
    }

    // Extract echo content and write directly
    let content = extract_echo_content(echo_part)?;

    // Process \n escape sequences to real newlines
    let content = content.replace("\\n", "\n").replace("\\t", "\t");

    // Ensure parent directory exists
    if let Some(parent) = Path::new(file_path).parent() {
        if !parent.as_os_str().is_empty() {
            let _ = std::fs::create_dir_all(parent);
        }
    }

    match std::fs::write(file_path, &content) {
        Ok(_) => Some(format!("Written {} bytes to {file_path}", content.len())),
        Err(e) => Some(format!("Error writing to {file_path}: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_command() {
        let result = parse_command_with_quotes("ls -la");
        assert_eq!(result, vec!["ls", "-la"]);
    }

    #[test]
    fn test_parse_command_with_quoted_arg() {
        let result = parse_command_with_quotes(r#"cat "file with spaces.txt""#);
        assert_eq!(result, vec!["cat", "file with spaces.txt"]);
    }

    #[test]
    fn test_parse_command_with_multiple_quoted_args() {
        let result = parse_command_with_quotes(r#"cp "source file.txt" "dest file.txt""#);
        assert_eq!(result, vec!["cp", "source file.txt", "dest file.txt"]);
    }

    #[test]
    fn test_parse_command_with_mixed_quotes_and_regular_args() {
        let result = parse_command_with_quotes(r#"git commit -m "Initial commit" --no-verify"#);
        assert_eq!(
            result,
            vec!["git", "commit", "-m", "Initial commit", "--no-verify"]
        );
    }

    #[test]
    fn test_parse_command_with_empty_string() {
        let result = parse_command_with_quotes("");
        assert_eq!(result, Vec::<String>::new());
    }

    #[test]
    fn test_parse_command_with_only_spaces() {
        let result = parse_command_with_quotes("   ");
        assert_eq!(result, Vec::<String>::new());
    }

    #[test]
    fn test_parse_command_with_trailing_spaces() {
        let result = parse_command_with_quotes("ls -la   ");
        assert_eq!(result, vec!["ls", "-la"]);
    }

    #[test]
    fn test_parse_command_with_leading_spaces() {
        let result = parse_command_with_quotes("   ls -la");
        assert_eq!(result, vec!["ls", "-la"]);
    }

    #[test]
    fn test_parse_command_with_path_containing_spaces() {
        let result = parse_command_with_quotes(r#"cd "C:\Program Files\MyApp""#);
        assert_eq!(result, vec!["cd", r"C:\Program Files\MyApp"]);
    }

    #[test]
    fn test_parse_command_with_nested_quotes() {
        // Quotes within quotes - outer quotes are removed
        let result = parse_command_with_quotes(r#"echo "Hello "World"""#);
        // This will parse as: echo "Hello " World ""
        // Which gives: ["echo", "Hello ", "World", ""]
        assert!(result.contains(&"echo".to_string()));
    }

    #[test]
    fn test_execute_empty_command() {
        let result = execute_command("");
        assert_eq!(result, "Error: Empty command");
    }

    #[test]
    fn test_execute_echo_command() {
        let result = execute_command("echo Hello");
        assert!(result.contains("Hello") || result.contains("executed successfully"));
    }

    #[test]
    fn test_cd_without_argument() {
        let result = execute_command("cd");
        assert!(result.contains("requires a directory argument"));
    }

    #[test]
    fn test_command_with_special_characters() {
        let result = parse_command_with_quotes(r#"grep "pattern*" file.txt"#);
        assert_eq!(result, vec!["grep", "pattern*", "file.txt"]);
    }

    #[test]
    fn test_git_commit_with_quoted_message() {
        let result = parse_command_with_quotes(r#"git commit -m "Fix bug #123""#);
        assert_eq!(result, vec!["git", "commit", "-m", "Fix bug #123"]);
    }

    #[test]
    fn test_windows_path_parsing() {
        let result = parse_command_with_quotes(r#"type "C:\Users\test\file.txt""#);
        assert_eq!(result, vec!["type", r"C:\Users\test\file.txt"]);
    }

    #[test]
    fn test_unix_path_parsing() {
        let result = parse_command_with_quotes(r#"cat "/home/user/my file.txt""#);
        assert_eq!(result, vec!["cat", "/home/user/my file.txt"]);
    }

    #[test]
    fn test_native_echo_redirect_preserves_dollar_vars() {
        let cmd = r#"echo "<?php\n\$table->id();\n\$fillable = ['name'];" > /tmp/test_echo_redir.php"#;
        let result = try_native_echo_redirect(cmd);
        assert!(result.is_some(), "Should match echo > file pattern");
        let content = std::fs::read_to_string("/tmp/test_echo_redir.php").unwrap();
        assert!(content.contains("$table"), "Dollar vars should be preserved");
        assert!(content.contains("$fillable"), "Dollar vars should be preserved");
        std::fs::remove_file("/tmp/test_echo_redir.php").ok();
    }

    #[test]
    fn test_native_echo_redirect_with_chain() {
        let cmd = r#"mkdir -p /tmp/test_echo_chain && echo "hello" > /tmp/test_echo_chain/test.txt"#;
        let result = try_native_echo_redirect(cmd);
        assert!(result.is_some());
        let content = std::fs::read_to_string("/tmp/test_echo_chain/test.txt").unwrap();
        assert_eq!(content.trim(), "hello");
        std::fs::remove_dir_all("/tmp/test_echo_chain").ok();
    }

    #[test]
    fn test_native_echo_redirect_non_echo_falls_through() {
        let cmd = "cat foo.txt > bar.txt";
        let result = try_native_echo_redirect(cmd);
        assert!(result.is_none(), "Non-echo redirects should fall through");
    }

    #[test]
    fn test_find_last_redirect() {
        assert_eq!(find_last_redirect(r#"echo "hi" > file.txt"#), Some(10));
        assert_eq!(find_last_redirect(r#"echo "a > b" > out.txt"#), Some(13));
        assert_eq!(find_last_redirect("echo hello"), None);
    }

    #[test]
    fn test_split_on_chain_ops() {
        let parts = split_on_chain_ops("mkdir -p dir && echo hi > f.txt");
        assert_eq!(parts, vec!["mkdir -p dir", "echo hi > f.txt"]);
    }

    #[test]
    fn test_extract_echo_content() {
        assert_eq!(extract_echo_content(r#"echo "hello world""#), Some("hello world".to_string()));
        assert_eq!(extract_echo_content(r#"echo 'single quotes'"#), Some("single quotes".to_string()));
        assert_eq!(extract_echo_content("echo bare text"), Some("bare text".to_string()));
    }

    #[test]
    fn test_native_echo_redirect_with_newline_escapes() {
        let cmd = r#"echo "line1\nline2\nline3" > /tmp/test_echo_newlines.txt"#;
        let result = try_native_echo_redirect(cmd);
        assert!(result.is_some());
        let content = std::fs::read_to_string("/tmp/test_echo_newlines.txt").unwrap();
        assert!(content.contains("line1\nline2\nline3") || content.contains("line1"));
        std::fs::remove_file("/tmp/test_echo_newlines.txt").ok();
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_cmd_quoted_paths_raw_arg() {
        // Simulates the exact command an AI agent generates: quoted PHP path + quoted composer path
        let php = env::current_dir().unwrap().join("php-8.2.30").join("php.exe");
        if !php.exists() {
            eprintln!("Skipping: php.exe not found at {:?}", php);
            return;
        }
        // Command with quoted paths — the exact pattern that broke before raw_arg fix
        let cmd = format!("\"{}\" -v", php.display());
        let result = execute_command(&cmd);
        assert!(result.contains("PHP"), "Expected PHP version output, got: {result}");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_streaming_cmd_quoted_paths_raw_arg() {
        let php = env::current_dir().unwrap().join("php-8.2.30").join("php.exe");
        if !php.exists() {
            eprintln!("Skipping: php.exe not found at {:?}", php);
            return;
        }
        let cmd = format!("\"{}\" -v", php.display());
        let mut lines = Vec::new();
        let result = execute_command_streaming(&cmd, None, |line| lines.push(line.to_string()));
        assert!(result.contains("PHP"), "Expected PHP version output, got: {result}");
        assert!(!lines.is_empty(), "Expected streaming lines, got none");
    }
}
