use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::Mutex;
use std::path::Path;

pub struct Logger {
    file: Mutex<File>,
}

impl Logger {
    pub fn new(log_path: &str) -> std::io::Result<Self> {
        // Create logs directory if it doesn't exist
        if let Some(parent) = Path::new(log_path).parent() {
            std::fs::create_dir_all(parent)?;
        }

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)?;

        Ok(Logger {
            file: Mutex::new(file),
        })
    }

    pub fn log(&self, level: &str, message: &str) {
        let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
        let log_line = format!("[{}] [{}] {}\n", timestamp, level, message);

        if let Ok(mut file) = self.file.lock() {
            let _ = file.write_all(log_line.as_bytes());
            let _ = file.flush();
        }
    }

    pub fn debug(&self, message: &str) {
        self.log("DEBUG", message);
    }

    pub fn info(&self, message: &str) {
        self.log("INFO", message);
    }

    pub fn warn(&self, message: &str) {
        self.log("WARN", message);
    }

    #[allow(dead_code)]
    pub fn error(&self, message: &str) {
        self.log("ERROR", message);
    }
}

// Global logger instance
lazy_static::lazy_static! {
    pub static ref LOGGER: Logger = Logger::new("logs/llama_chat.log")
        .expect("Failed to create logger");
}

// Convenience macros
#[macro_export]
macro_rules! log_debug {
    ($($arg:tt)*) => {
        $crate::web::logger::LOGGER.debug(&format!($($arg)*));
    };
}

#[macro_export]
macro_rules! log_info {
    ($($arg:tt)*) => {
        $crate::web::logger::LOGGER.info(&format!($($arg)*));
    };
}

#[macro_export]
macro_rules! log_warn {
    ($($arg:tt)*) => {
        $crate::web::logger::LOGGER.warn(&format!($($arg)*));
    };
}

#[macro_export]
macro_rules! log_error {
    ($($arg:tt)*) => {
        $crate::web::logger::LOGGER.error(&format!($($arg)*));
    };
}
