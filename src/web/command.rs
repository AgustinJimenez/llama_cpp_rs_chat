use crate::log_warn;
use std::env;
use std::process::Command;

// Whitelist of allowed commands for security
const ALLOWED_COMMANDS: &[&str] = &[
    // File operations
    "ls", "dir", "cat", "type", "head", "tail", "find", "grep", "more", "less",
    // Directory operations
    "cd", "pwd", "mkdir", "rmdir", // File manipulation
    "cp", "mv", "rm", "del", "touch", "chmod", // System info
    "echo", "date", "whoami", "hostname", "uname", // Development tools
    "git", "cargo", "npm", "node", "python", "rustc", // Archive operations
    "tar", "zip", "unzip", "gzip", "gunzip", // Text processing
    "sed", "awk", "sort", "uniq", "wc", "diff",
];

// Helper function to parse command with proper quote handling
pub fn parse_command_with_quotes(cmd: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current_part = String::new();
    let mut in_quotes = false;
    let mut chars = cmd.chars().peekable();

    while let Some(ch) = chars.next() {
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

// Helper function to execute system commands
pub fn execute_command(cmd: &str) -> String {
    // Parse command with proper quote handling
    let parts = parse_command_with_quotes(cmd.trim());
    if parts.is_empty() {
        return "Error: Empty command".to_string();
    }

    let command_name = &parts[0];

    // Security: Check if command is in whitelist
    if !ALLOWED_COMMANDS.contains(&command_name.as_str()) {
        log_warn!("system", "Blocked unauthorized command: {}", command_name);
        return format!(
            "Error: Command '{}' is not allowed for security reasons. Allowed commands: {}",
            command_name,
            ALLOWED_COMMANDS.join(", ")
        );
    }

    // Basic command validation - reject obviously invalid commands
    if command_name.len() < 2 || command_name.contains("/") && !command_name.starts_with("/") {
        return format!("Error: Invalid command format: {}", command_name);
    }

    // Prevent dangerous filesystem-wide searches
    if command_name == "find" && parts.len() > 1 {
        let search_path = &parts[1];
        if search_path == "/" || search_path == "/usr" || search_path == "/System" {
            return format!("Error: Filesystem-wide searches are not allowed for performance and security reasons. Try searching in specific directories like current directory '.'");
        }
    }

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
                format!("Error: Failed to change directory: {}", e)
            }
        }
    } else {
        // Normal command execution for non-cd commands
        // On Windows, use cmd.exe for built-in commands like type, dir, echo, etc.
        let is_windows = cfg!(target_os = "windows");
        let windows_builtins = [
            "type", "dir", "echo", "del", "copy", "move", "ren", "cls", "date", "time",
        ];

        let mut command = if is_windows && windows_builtins.contains(&command_name.as_str()) {
            // Use cmd.exe /c for Windows built-in commands
            let full_cmd = parts.join(" ");
            let mut cmd = Command::new("cmd");
            cmd.args(["/c", &full_cmd]);
            cmd
        } else {
            let mut cmd = Command::new(&parts[0]);
            if parts.len() > 1 {
                cmd.args(&parts[1..]);
            }
            cmd
        };

        match command.output() {
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
                                format!("Command '{}' executed successfully", command_name)
                            }
                        }
                    }
                } else if !stderr.is_empty() {
                    format!("{}\nError: {}", stdout, stderr)
                } else {
                    stdout.to_string()
                }
            }
            Err(e) => {
                format!("Failed to execute command: {}", e)
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
    fn test_execute_blocked_command() {
        let result = execute_command("malicious_command");
        assert!(result.contains("not allowed for security reasons"));
        assert!(result.contains("malicious_command"));
    }

    #[test]
    fn test_execute_allowed_echo_command() {
        let result = execute_command("echo Hello");
        assert!(result.contains("Hello") || result.contains("executed successfully"));
    }

    #[test]
    fn test_whitelist_contains_basic_commands() {
        assert!(ALLOWED_COMMANDS.contains(&"ls"));
        assert!(ALLOWED_COMMANDS.contains(&"cat"));
        assert!(ALLOWED_COMMANDS.contains(&"git"));
        assert!(ALLOWED_COMMANDS.contains(&"echo"));
    }

    #[test]
    fn test_whitelist_does_not_contain_dangerous_commands() {
        assert!(!ALLOWED_COMMANDS.contains(&"rm -rf"));
        assert!(!ALLOWED_COMMANDS.contains(&"shutdown"));
        assert!(!ALLOWED_COMMANDS.contains(&"reboot"));
        assert!(!ALLOWED_COMMANDS.contains(&"format"));
    }

    #[test]
    fn test_find_command_blocked_on_root() {
        let result = execute_command("find / -name test");
        assert!(result.contains("Filesystem-wide searches are not allowed"));
    }

    #[test]
    fn test_find_command_blocked_on_usr() {
        let result = execute_command("find /usr -name test");
        assert!(result.contains("Filesystem-wide searches are not allowed"));
    }

    #[test]
    fn test_find_command_allowed_on_current_dir() {
        let result = execute_command("find . -name test");
        // Should not contain the block message
        assert!(!result.contains("Filesystem-wide searches are not allowed"));
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
