use std::env;
use std::process::Command;

use super::logger::ConversationLogger;

pub(crate) fn detect_and_execute_command(
    text: &str,
    conversation_logger: &mut ConversationLogger,
    show_command_output: bool,
    debug_test: bool,
) -> (String, bool) {
    if let Some(start) = text.find("<function_calls>") {
        if let Some(end) = text.find("</function_calls>") {
            if end > start {
                let function_block = &text[start..end + 16];

                if function_block.contains("execute_command") {
                    if let Some(param_start) = function_block.find("<parameter name=\"command\">") {
                        if let Some(param_end) = function_block.find("</parameter>") {
                            if param_end > param_start {
                                let command_text = &function_block[param_start + 26..param_end];
                                let before_command = &text[..start];
                                let output = execute_command(command_text, debug_test);
                                conversation_logger
                                    .log_command_execution(command_text, &output);

                                if show_command_output {
                                    println!("\n[Executing function: execute_command]");
                                    println!("[Command: {command_text}]");
                                    println!("[Output:]");
                                    println!("{output}");
                                    println!("[End of output]\n");
                                }

                                let new_text = format!(
                                    "{before_command}[Function executed: execute_command({command_text})]\n[Output:]\n{output}\n[/Output]\n\nBased on this output: "
                                );
                                return (new_text, true);
                            }
                        }
                    }
                }
            }
        }
    }

    if let Some(start) = text.find("<COMMAND>") {
        if let Some(end) = text.find("</COMMAND>") {
            if end > start {
                let command_text = &text[start + 9..end];
                let before_command = &text[..start];
                let output = execute_command(command_text, debug_test);
                conversation_logger.log_command_execution(command_text, &output);

                if show_command_output {
                    println!("\n[Executing command: {command_text}]");
                    println!("[Command output:]");
                    println!("{output}");
                    println!("[End of command output]\n");
                }

                let new_text = format!(
                    "{before_command}[Command executed: {command_text}]\n[Output:]\n{output}\n[/Output]\n\nBased on this output: "
                );
                return (new_text, true);
            }
        }
    }

    (text.to_string(), false)
}

fn parse_command_with_quotes(cmd: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current_part = String::new();
    let mut in_quotes = false;

    for ch in cmd.chars() {
        match ch {
            '"' => in_quotes = !in_quotes,
            ' ' if !in_quotes => {
                if !current_part.is_empty() {
                    parts.push(current_part.clone());
                    current_part.clear();
                }
            }
            _ => current_part.push(ch),
        }
    }

    if !current_part.is_empty() {
        parts.push(current_part);
    }

    parts
}

fn execute_command(cmd: &str, debug_test: bool) -> String {
    let parts = parse_command_with_quotes(cmd.trim());
    if parts.is_empty() {
        return "Error: Empty command".to_string();
    }

    let command_name = &parts[0];
    if command_name.len() < 2 || command_name.contains('/') && !command_name.starts_with('/') {
        return format!("Error: Invalid command format: {command_name}");
    }

    if command_name == "find" && parts.len() > 1 {
        let search_path = &parts[1];
        if search_path == "/" || search_path == "/usr" || search_path == "/System" {
            return "Error: Filesystem-wide searches are not allowed for performance and security reasons. Try searching in specific directories like /Users/$USER, ~/.local, or current directory '.'".to_string();
        }
    }

    if command_name == "cd" {
        if debug_test {
            eprintln!("DEBUG: Executing cd command: {cmd:?}");
            eprintln!("DEBUG: Command parts: {parts:?}");
        }

        let Some(target_dir) = parts.get(1) else {
            return "Error: cd command requires a directory argument".to_string();
        };

        return match env::set_current_dir(target_dir) {
            Ok(_) => match env::current_dir() {
                Ok(new_dir) => format!("Successfully changed directory to: {}", new_dir.display()),
                Err(_) => "Directory changed successfully".to_string(),
            },
            Err(e) => format!("Error: Failed to change directory: {e}"),
        };
    }

    let mut command = Command::new(command_name);
    if parts.len() > 1 {
        command.args(&parts[1..]);
    }

    if debug_test {
        eprintln!("DEBUG: Executing command: {cmd:?}");
        eprintln!("DEBUG: Command parts: {parts:?}");
    }

    match command.output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);

            if output.status.success() && stdout.is_empty() && stderr.is_empty() {
                match command_name.as_str() {
                    "find" => "No files found matching the search criteria".to_string(),
                    "mkdir" => "Directory created successfully".to_string(),
                    "touch" => "File created successfully".to_string(),
                    "rm" | "rmdir" => "File/directory removed successfully".to_string(),
                    "mv" | "cp" => "File operation completed successfully".to_string(),
                    "chmod" => "Permissions changed successfully".to_string(),
                    _ if parts.len() > 1 => {
                        format!("Command '{}' executed successfully", parts.join(" "))
                    }
                    _ => format!("Command '{command_name}' executed successfully"),
                }
            } else if !stderr.is_empty() {
                format!("{stdout}\nError: {stderr}")
            } else {
                stdout.to_string()
            }
        }
        Err(e) => {
            if debug_test {
                eprintln!("DEBUG: Command execution failed: {e}");
            }
            format!("Failed to execute command: {e}")
        }
    }
}
