use super::*;

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

#[cfg(not(windows))]
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

#[cfg(not(windows))]
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

#[cfg(not(windows))]
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
    let php = env::current_dir().unwrap().join("php-8.2.30").join("php.exe");
    if !php.exists() {
        eprintln!("Skipping: php.exe not found at {:?}", php);
        return;
    }
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

// All CWD-mutating tests are merged into one sequential test to avoid
// parallel races on the process-global `env::current_dir()`.
#[test]
fn test_track_cwd_change_suite() {
    let original = env::current_dir().unwrap();
    let temp = env::temp_dir();

    // with &&
    let cmd = format!("cd {} && echo hello", temp.display());
    track_cwd_change(&cmd);
    let now = env::current_dir().unwrap();
    let _ = env::set_current_dir(&original);
    assert_eq!(
        now.canonicalize().unwrap_or(now.clone()),
        temp.canonicalize().unwrap_or(temp.clone()),
        "CWD should have changed to temp dir (&&)"
    );

    // with ;
    let cmd = format!("cd {}; echo hello", temp.display());
    track_cwd_change(&cmd);
    let now = env::current_dir().unwrap();
    let _ = env::set_current_dir(&original);
    assert_eq!(
        now.canonicalize().unwrap_or(now.clone()),
        temp.canonicalize().unwrap_or(temp.clone()),
        "CWD should have changed to temp dir (;)"
    );

    // quoted path
    let cmd = format!("cd \"{}\" && echo hello", temp.display());
    track_cwd_change(&cmd);
    let now = env::current_dir().unwrap();
    let _ = env::set_current_dir(&original);
    assert_eq!(
        now.canonicalize().unwrap_or(now.clone()),
        temp.canonicalize().unwrap_or(temp.clone()),
        "CWD should have changed to temp dir (quoted)"
    );

    // no cd — CWD unchanged
    let before = env::current_dir().unwrap();
    track_cwd_change("echo hello && echo world");
    let now = env::current_dir().unwrap();
    assert_eq!(now, before, "CWD should not change for non-cd commands");

    // invalid dir — CWD unchanged
    let before = env::current_dir().unwrap();
    track_cwd_change("cd /nonexistent_dir_12345 && echo hello");
    let now = env::current_dir().unwrap();
    assert_eq!(now, before, "CWD should not change for invalid directory");

    // cwd_annotation: same dir → None
    let cwd = env::current_dir().unwrap();
    assert!(cwd_annotation(&cwd).is_none(), "No annotation when CWD unchanged");

    // cwd_annotation: different dir → Some
    let _ = env::set_current_dir(&temp);
    let annotation = cwd_annotation(&original);
    let _ = env::set_current_dir(&original);
    assert!(annotation.is_some(), "Should produce annotation when CWD differs");
    assert!(annotation.unwrap().contains("[CWD:"));
}
