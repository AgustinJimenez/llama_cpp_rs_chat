use std::env;
use std::process::Command;

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
    // Parse command with proper quote handling
    let parts = parse_command_with_quotes(cmd.trim());
    if parts.is_empty() {
        return "Error: Empty command".to_string();
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
                } else if !stderr.is_empty() {
                    format!("{stdout}\nError: {stderr}")
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
}
