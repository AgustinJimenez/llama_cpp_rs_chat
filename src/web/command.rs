use std::process::Command;
use std::env;

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
        let mut command = Command::new(&parts[0]);
        if parts.len() > 1 {
            command.args(&parts[1..]);
        }

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
