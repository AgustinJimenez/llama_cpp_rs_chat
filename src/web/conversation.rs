use std::fs;
use std::io;
use super::models::ChatMessage;

// Helper function to get current timestamp for logging
pub fn timestamp_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap();
    let secs = now.as_secs();
    let millis = now.subsec_millis();
    let hours = (secs / 3600) % 24;
    let minutes = (secs / 60) % 60;
    let seconds = secs % 60;
    format!("{:02}:{:02}:{:02}.{:03}", hours, minutes, seconds, millis)
}

pub struct ConversationLogger {
    file_path: String,
    content: String,
}

impl ConversationLogger {
    pub fn new(system_prompt: Option<&str>) -> io::Result<Self> {
        // Create assets/conversations directory if it doesn't exist
        let conversations_dir = "assets/conversations";
        fs::create_dir_all(conversations_dir)?;

        // Generate timestamp-based filename with YYYY-MM-DD-HH-mm-ss-SSS format
        let now = std::time::SystemTime::now();
        let since_epoch = now
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        // Convert to a more readable format
        let secs = since_epoch.as_secs();
        let millis = since_epoch.subsec_millis();

        // Simple conversion (this won't be perfect timezone-wise, but it's readable)
        let days_since_epoch = secs / 86400;
        let remaining_secs = secs % 86400;
        let hours = remaining_secs / 3600;
        let remaining_secs = remaining_secs % 3600;
        let minutes = remaining_secs / 60;
        let seconds = remaining_secs % 60;

        // Approximate date calculation (starting from 1970-01-01)
        let year = 1970 + (days_since_epoch / 365);
        let day_of_year = days_since_epoch % 365;
        let month = std::cmp::min(12, (day_of_year / 30) + 1);
        let day = (day_of_year % 30) + 1;

        let timestamp = format!(
            "{:04}-{:02}-{:02}-{:02}-{:02}-{:02}-{:03}",
            year, month, day, hours, minutes, seconds, millis
        );

        let file_path = format!("{}/chat_{}.txt", conversations_dir, timestamp);

        let mut logger = ConversationLogger {
            file_path,
            content: String::new(),
        };

        // Only log system prompt if one is explicitly provided
        // If None, the model's chat template will use its built-in default
        if let Some(prompt) = system_prompt {
            logger.log_message("SYSTEM", prompt);
        }

        Ok(logger)
    }

    pub fn from_existing(conversation_id: &str) -> io::Result<Self> {
        // Load existing conversation file
        let conversations_dir = "assets/conversations";

        // Handle .txt extension if already present
        let file_path = if conversation_id.ends_with(".txt") {
            format!("{}/{}", conversations_dir, conversation_id)
        } else {
            format!("{}/{}.txt", conversations_dir, conversation_id)
        };

        // Read existing content
        let content = fs::read_to_string(&file_path)?;

        Ok(ConversationLogger {
            file_path,
            content,
        })
    }

    pub fn log_message(&mut self, role: &str, message: &str) {
        let log_entry = format!("{}:\n{}\n\n", role, message);
        self.content.push_str(&log_entry);

        // Write immediately to file
        if let Err(e) = fs::write(&self.file_path, &self.content) {
            eprintln!("Failed to write conversation log: {}", e);
        }
    }

    pub fn log_token(&mut self, token: &str) {
        // Append token to the last assistant message in content
        self.content.push_str(token);

        eprintln!("[{}] [LOGGER] Writing token to file: {} (token: '{}', total content length: {})",
            timestamp_now(), self.file_path,
            if token.len() > 50 { &token[..50] } else { token }, self.content.len());

        // Write to file immediately so file watcher can update UI in real-time
        // This is now fast enough since we're not blocking on WebSocket
        if let Err(e) = fs::write(&self.file_path, &self.content) {
            eprintln!("[{}] [LOGGER] ERROR: Failed to write conversation log: {}",
                timestamp_now(), e);
        } else {
            eprintln!("[{}] [LOGGER] File written successfully",
                timestamp_now());
        }
    }

    pub fn finish_assistant_message(&mut self) {
        // Add proper newlines after assistant message completion
        self.content.push_str("\n\n");

        // Write immediately to file
        if let Err(e) = fs::write(&self.file_path, &self.content) {
            eprintln!("Failed to write conversation log: {}", e);
        }
    }

    pub fn get_conversation_id(&self) -> String {
        // Extract filename from path (e.g., "assets/conversations/chat_2025-01-15-10-30-45-123.txt" -> "chat_2025-01-15-10-30-45-123.txt")
        std::path::Path::new(&self.file_path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("")
            .to_string()
    }

    pub fn log_command_execution(&mut self, command: &str, output: &str) {
        let log_entry = format!("[COMMAND: {}]\n{}\n\n", command, output);
        self.content.push_str(&log_entry);

        // Write immediately to file
        if let Err(e) = fs::write(&self.file_path, &self.content) {
            eprintln!("Failed to write conversation log: {}", e);
        }
    }

    pub fn get_full_conversation(&self) -> String {
        // Return the complete conversation content from memory
        self.content.clone()
    }

    pub fn load_conversation_from_file(&self) -> io::Result<String> {
        // Read the conversation directly from file (source of truth)
        fs::read_to_string(&self.file_path)
    }
}

pub fn parse_conversation_to_messages(conversation: &str) -> Vec<ChatMessage> {
    let mut messages = Vec::new();
    let mut current_role = "";
    let mut current_content = String::new();

    for line in conversation.lines() {
        if line.ends_with(":")
            && (line.starts_with("SYSTEM:")
                || line.starts_with("USER:")
                || line.starts_with("ASSISTANT:"))
        {
            // Save previous message if it exists
            if !current_role.is_empty() && !current_content.trim().is_empty() {
                let role = match current_role {
                    "SYSTEM" => "system",
                    "USER" => "user",
                    "ASSISTANT" => "assistant",
                    _ => "user",
                };

                // Skip system messages in the UI
                if role != "system" {
                    messages.push(ChatMessage {
                        id: uuid::Uuid::new_v4().to_string(),
                        role: role.to_string(),
                        content: current_content.trim().to_string(),
                        timestamp: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs(),
                    });
                }
            }

            // Start new message
            current_role = line.trim_end_matches(":");
            current_content.clear();
        } else if !line.starts_with("[COMMAND:") && !line.trim().is_empty() {
            // Skip command execution logs, add content
            current_content.push_str(line);
            current_content.push('\n');
        }
    }

    // Add the final message
    if !current_role.is_empty() && !current_content.trim().is_empty() {
        let role = match current_role {
            "SYSTEM" => "system",
            "USER" => "user",
            "ASSISTANT" => "assistant",
            _ => "user",
        };

        // Skip system messages in the UI
        if role != "system" {
            messages.push(ChatMessage {
                id: uuid::Uuid::new_v4().to_string(),
                role: role.to_string(),
                content: current_content.trim().to_string(),
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            });
        }
    }

    messages
}
