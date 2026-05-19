use std::fs;
use std::io;

pub(crate) struct ConversationLogger {
    file_path: String,
    content: String,
}

impl ConversationLogger {
    pub(crate) fn new() -> io::Result<Self> {
        let conversations_dir = "assets/conversations";
        fs::create_dir_all(conversations_dir)?;

        let now = std::time::SystemTime::now();
        let since_epoch = now
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(io::Error::other)?;

        let secs = since_epoch.as_secs();
        let millis = since_epoch.subsec_millis();

        let days_since_epoch = secs / 86400;
        let remaining_secs = secs % 86400;
        let hours = remaining_secs / 3600;
        let remaining_secs = remaining_secs % 3600;
        let minutes = remaining_secs / 60;
        let seconds = remaining_secs % 60;

        let year = 1970 + (days_since_epoch / 365);
        let day_of_year = days_since_epoch % 365;
        let month = std::cmp::min(12, (day_of_year / 30) + 1);
        let day = (day_of_year % 30) + 1;

        let timestamp = format!(
            "{:04}-{:02}-{:02}-{:02}-{:02}-{:02}-{:03}",
            year, month, day, hours, minutes, seconds, millis
        );

        let file_path = format!("{}/chat_{}.txt", conversations_dir, timestamp);

        Ok(Self {
            file_path,
            content: String::new(),
        })
    }

    pub(crate) fn log_message(&mut self, role: &str, message: &str) {
        let log_entry = format!("{role}:\n{message}\n\n");
        self.content.push_str(&log_entry);

        if let Err(e) = fs::write(&self.file_path, &self.content) {
            eprintln!("Failed to write conversation log: {e}");
        }
    }

    pub(crate) fn log_command_execution(&mut self, command: &str, output: &str) {
        let log_entry = format!("[COMMAND: {command}]\n{output}\n\n");
        self.content.push_str(&log_entry);

        if let Err(e) = fs::write(&self.file_path, &self.content) {
            eprintln!("Failed to write conversation log: {e}");
        }
    }

    pub(crate) fn save(&self) -> io::Result<()> {
        fs::write(&self.file_path, &self.content)?;
        println!("Conversation saved to: {}", self.file_path);
        Ok(())
    }
}
