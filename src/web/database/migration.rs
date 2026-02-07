// Migration from file-based storage to SQLite

use super::config::DbSamplerConfig;
use super::Database;
use std::fs;
use std::path::Path;

// Import logging macro
use crate::sys_warn;

/// Migrate existing conversation files to SQLite
pub fn migrate_existing_conversations(db: &Database) -> Result<u32, String> {
    let conversations_dir = Path::new("assets/conversations");
    if !conversations_dir.exists() {
        return Ok(0);
    }

    let mut migrated_count = 0;

    let entries = fs::read_dir(conversations_dir)
        .map_err(|e| format!("Failed to read conversations directory: {e}"))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("Failed to read directory entry: {e}"))?;
        let path = entry.path();

        if path.extension().is_some_and(|ext| ext == "txt") {
            if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
                if filename.starts_with("chat_") {
                    let conv_id = filename.trim_end_matches(".txt").to_string();

                    // Check if already migrated
                    if !db.conversation_exists(&conv_id)? {
                        if let Err(e) = migrate_single_conversation(db, &path, &conv_id) {
                            sys_warn!("Warning: Failed to migrate {}: {}", conv_id, e);
                            continue;
                        }
                        migrated_count += 1;
                    }
                }
            }
        }
    }

    Ok(migrated_count)
}

/// Migrate a single conversation file
fn migrate_single_conversation(
    db: &Database,
    file_path: &Path,
    conv_id: &str,
) -> Result<(), String> {
    let content =
        fs::read_to_string(file_path).map_err(|e| format!("Failed to read file: {e}"))?;

    // Parse timestamp from conversation ID: chat_YYYY-MM-DD-HH-mm-ss-SSS
    let created_at = parse_timestamp_from_id(conv_id).unwrap_or_else(|| {
        // Fallback to file modification time
        file_path
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or_else(super::current_timestamp_millis)
    });

    // Parse messages from file content
    let messages = parse_conversation_content(&content);

    // Get system prompt (first SYSTEM message)
    let system_prompt = messages
        .iter()
        .find(|(role, _)| role == "SYSTEM")
        .map(|(_, content)| content.clone());

    // Insert conversation
    db.create_conversation_with_id(conv_id, system_prompt.as_deref(), created_at)?;

    // Insert messages (skip system prompt as it's stored in conversation)
    let mut sequence = 0;
    let base_timestamp = (created_at / 1000) as u64; // Convert to seconds

    for (role, content) in messages {
        if role == "SYSTEM" {
            // System message is already in conversation record
            continue;
        }

        let role_lower = role.to_lowercase();
        let msg_timestamp = base_timestamp + sequence as u64;

        db.insert_message(conv_id, &role_lower, &content, msg_timestamp, sequence)?;
        sequence += 1;
    }

    Ok(())
}

/// Parse conversation ID timestamp: chat_YYYY-MM-DD-HH-mm-ss-SSS
fn parse_timestamp_from_id(conv_id: &str) -> Option<i64> {
    // Format: chat_2025-01-15-14-30-45-123
    let parts: Vec<&str> = conv_id.trim_start_matches("chat_").split('-').collect();

    if parts.len() < 7 {
        return None;
    }

    let year: i32 = parts[0].parse().ok()?;
    let month: u32 = parts[1].parse().ok()?;
    let day: u32 = parts[2].parse().ok()?;
    let hour: u32 = parts[3].parse().ok()?;
    let minute: u32 = parts[4].parse().ok()?;
    let second: u32 = parts[5].parse().ok()?;
    let millis: u32 = parts[6].parse().ok()?;

    // Convert to timestamp (approximate)
    // Days from year
    let mut days: i64 = 0;
    for y in 1970..year {
        days += if is_leap_year(y as i64) { 366 } else { 365 };
    }

    // Days from months
    let days_in_months: [i64; 12] = if is_leap_year(year as i64) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    for dim in &days_in_months[..(month - 1) as usize] {
        days += dim;
    }

    // Add day of month
    days += (day - 1) as i64;

    // Calculate total seconds
    let total_secs = days * 86400 + hour as i64 * 3600 + minute as i64 * 60 + second as i64;

    // Return milliseconds
    Some(total_secs * 1000 + millis as i64)
}

fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

/// Parse conversation file content into (role, content) pairs
fn parse_conversation_content(content: &str) -> Vec<(String, String)> {
    let mut messages = Vec::new();
    let mut current_role = String::new();
    let mut current_content = String::new();

    for line in content.lines() {
        // Check for role headers
        if line == "SYSTEM:" || line == "USER:" || line == "ASSISTANT:" {
            // Save previous message if any
            if !current_role.is_empty() {
                let trimmed = current_content.trim().to_string();
                if !trimmed.is_empty() {
                    messages.push((current_role.clone(), trimmed));
                }
            }
            current_role = line.trim_end_matches(':').to_string();
            current_content.clear();
        } else if line.starts_with("[COMMAND:") {
            // Skip old command format lines
            continue;
        } else {
            // Accumulate content
            current_content.push_str(line);
            current_content.push('\n');
        }
    }

    // Don't forget the last message
    if !current_role.is_empty() {
        let trimmed = current_content.trim().to_string();
        if !trimmed.is_empty() {
            messages.push((current_role, trimmed));
        }
    }

    messages
}

/// Migrate config.json to SQLite
pub fn migrate_config(db: &Database) -> Result<bool, String> {
    let config_path = Path::new("assets/config.json");
    if !config_path.exists() {
        return Ok(false);
    }

    // Check if config already has data (don't overwrite)
    let current_config = db.load_config();
    if current_config.model_path.is_some() {
        // Config already has data, don't overwrite
        return Ok(false);
    }

    // Read and parse JSON config
    let content = fs::read_to_string(config_path)
        .map_err(|e| format!("Failed to read config.json: {e}"))?;

    #[derive(serde::Deserialize)]
    struct JsonConfig {
        sampler_type: Option<String>,
        temperature: Option<f64>,
        top_p: Option<f64>,
        top_k: Option<u32>,
        mirostat_tau: Option<f64>,
        mirostat_eta: Option<f64>,
        model_path: Option<String>,
        system_prompt: Option<String>,
        context_size: Option<u32>,
        stop_tokens: Option<Vec<String>>,
        model_history: Option<Vec<String>>,
    }

    let json_config: JsonConfig = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse config.json: {e}"))?;

    // Convert to DbSamplerConfig
    let db_config = DbSamplerConfig {
        sampler_type: json_config
            .sampler_type
            .unwrap_or_else(|| "Greedy".to_string()),
        temperature: json_config.temperature.unwrap_or(0.7),
        top_p: json_config.top_p.unwrap_or(0.95),
        top_k: json_config.top_k.unwrap_or(20),
        mirostat_tau: json_config.mirostat_tau.unwrap_or(5.0),
        mirostat_eta: json_config.mirostat_eta.unwrap_or(0.1),
        model_path: json_config.model_path,
        system_prompt: json_config.system_prompt,
        context_size: json_config.context_size,
        stop_tokens: json_config.stop_tokens,
        model_history: Vec::new(), // Handled separately
    };

    // Save to database
    db.save_config(&db_config)?;

    // Migrate model history
    if let Some(history) = json_config.model_history {
        // Insert in reverse order so most recent is at position 0
        for model_path in history.iter().rev() {
            let _ = db.add_to_model_history(model_path);
        }
    }

    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn create_test_db() -> Arc<Database> {
        Arc::new(Database::new(":memory:").unwrap())
    }

    #[test]
    fn test_parse_timestamp_from_id() {
        let id = "chat_2025-01-15-14-30-45-123";
        let ts = parse_timestamp_from_id(id);
        assert!(ts.is_some());

        // Verify it's a reasonable timestamp (after 2024)
        let ts = ts.unwrap();
        assert!(ts > 1704067200000); // 2024-01-01 00:00:00 UTC in millis
    }

    #[test]
    fn test_parse_conversation_content() {
        let content = r#"SYSTEM:
You are a helpful assistant.

USER:
Hello, how are you?

ASSISTANT:
I'm doing well, thank you!

USER:
Great!
"#;

        let messages = parse_conversation_content(content);

        assert_eq!(messages.len(), 4);
        assert_eq!(messages[0].0, "SYSTEM");
        assert_eq!(messages[0].1, "You are a helpful assistant.");
        assert_eq!(messages[1].0, "USER");
        assert_eq!(messages[1].1, "Hello, how are you?");
        assert_eq!(messages[2].0, "ASSISTANT");
        assert_eq!(messages[2].1, "I'm doing well, thank you!");
        assert_eq!(messages[3].0, "USER");
        assert_eq!(messages[3].1, "Great!");
    }

    #[test]
    fn test_parse_conversation_with_commands() {
        let content = r#"USER:
List files

ASSISTANT:
<||SYSTEM.EXEC>ls<SYSTEM.EXEC||>
[COMMAND: ls]
file1.txt
file2.txt

Here are the files.
"#;

        let messages = parse_conversation_content(content);

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].0, "USER");
        assert_eq!(messages[1].0, "ASSISTANT");
        // Command lines should be included, [COMMAND:] lines skipped
        assert!(messages[1].1.contains("<||SYSTEM.EXEC>"));
        assert!(!messages[1].1.contains("[COMMAND:"));
    }

    #[test]
    fn test_migrate_config_to_db() {
        let db = create_test_db();

        // Create a temporary config file for testing
        // (In real test, would use tempfile crate)

        // Just test that default config works
        let config = db.load_config();
        assert_eq!(config.sampler_type, "Greedy");
    }
}
