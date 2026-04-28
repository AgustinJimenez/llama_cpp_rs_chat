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
pub fn needs_shell(cmd: &str) -> bool {
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

/// Find the position of the last `>` redirect operator that is NOT inside quotes.
pub fn find_last_redirect(cmd: &str) -> Option<usize> {
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
pub fn split_on_chain_ops(cmd: &str) -> Vec<&str> {
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
pub fn extract_echo_content(echo_part: &str) -> Option<String> {
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
        let result = parse_command_with_quotes(r#"echo "Hello "World"""#);
        assert!(result.contains(&"echo".to_string()));
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
}
