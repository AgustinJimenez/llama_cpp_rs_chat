use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::Mutex;
use std::collections::HashMap;

pub struct Logger {
    files: Mutex<HashMap<String, File>>,
}

impl Logger {
    pub fn new() -> Self {
        Logger {
            files: Mutex::new(HashMap::new()),
        }
    }

    fn get_or_create_file(&self, conversation_id: &str) -> std::io::Result<()> {
        let mut files = self.files.lock().unwrap();

        if !files.contains_key(conversation_id) {
            // Create logs directory if it doesn't exist
            let log_dir = "logs/conversations";
            std::fs::create_dir_all(log_dir)?;

            // Create log file path based on conversation ID
            let log_path = format!("{}/{}.log", log_dir, conversation_id.replace(".txt", ""));

            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log_path)?;

            files.insert(conversation_id.to_string(), file);
        }

        Ok(())
    }

    pub fn log(&self, conversation_id: &str, level: &str, message: &str) {
        // Create or get the file for this conversation
        if let Err(e) = self.get_or_create_file(conversation_id) {
            // Can't use logging macros here as this IS the logger - use eprintln as fallback
            eprintln!("LOGGER ERROR: Failed to create log file for {}: {}", conversation_id, e);
            return;
        }

        let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
        let log_line = format!("[{}] [{}] {}\n", timestamp, level, message);

        if let Ok(mut files) = self.files.lock() {
            if let Some(file) = files.get_mut(conversation_id) {
                let _ = file.write_all(log_line.as_bytes());
                let _ = file.flush();
            }
        }
    }

    pub fn debug(&self, conversation_id: &str, message: &str) {
        self.log(conversation_id, "DEBUG", message);
    }

    pub fn info(&self, conversation_id: &str, message: &str) {
        self.log(conversation_id, "INFO", message);
    }

    pub fn warn(&self, conversation_id: &str, message: &str) {
        self.log(conversation_id, "WARN", message);
    }

    #[allow(dead_code)]
    pub fn error(&self, conversation_id: &str, message: &str) {
        self.log(conversation_id, "ERROR", message);
    }
}

// Global logger instance
lazy_static::lazy_static! {
    pub static ref LOGGER: Logger = Logger::new();
}

// Convenience macros - now require conversation_id as first parameter
#[macro_export]
macro_rules! log_debug {
    ($conv_id:expr, $($arg:tt)*) => {
        $crate::web::logger::LOGGER.debug($conv_id, &format!($($arg)*));
    };
}

#[macro_export]
macro_rules! log_info {
    ($conv_id:expr, $($arg:tt)*) => {
        $crate::web::logger::LOGGER.info($conv_id, &format!($($arg)*));
    };
}

#[macro_export]
macro_rules! log_warn {
    ($conv_id:expr, $($arg:tt)*) => {
        $crate::web::logger::LOGGER.warn($conv_id, &format!($($arg)*));
    };
}

#[macro_export]
macro_rules! log_error {
    ($conv_id:expr, $($arg:tt)*) => {
        $crate::web::logger::LOGGER.error($conv_id, &format!($($arg)*));
    };
}

// System-level logging macros (for logs without conversation context)
#[macro_export]
macro_rules! sys_debug {
    ($($arg:tt)*) => {
        $crate::web::logger::LOGGER.debug("system", &format!($($arg)*));
    };
}

#[macro_export]
macro_rules! sys_info {
    ($($arg:tt)*) => {
        $crate::web::logger::LOGGER.info("system", &format!($($arg)*));
    };
}

#[macro_export]
macro_rules! sys_warn {
    ($($arg:tt)*) => {
        $crate::web::logger::LOGGER.warn("system", &format!($($arg)*));
    };
}

#[macro_export]
macro_rules! sys_error {
    ($($arg:tt)*) => {
        $crate::web::logger::LOGGER.error("system", &format!($($arg)*));
    };
}
