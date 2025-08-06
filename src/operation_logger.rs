use crate::ai_operations::*;
use anyhow::Result;
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Mutex;
use serde_json;

pub enum LoggerType {
    Console,
    Json,
    // Add other types like Csv, Database, etc.
}

pub struct LoggerConfig {
    pub logger_type: LoggerType,
    pub log_path: Option<PathBuf>,
    pub max_entries: Option<usize>,
}

pub struct JsonFileLogger {
    log_path: PathBuf,
    max_entries: Option<usize>,
    // Using a Mutex to allow interior mutability for logging
    // This is a simple approach; for high-concurrency, consider channels or a dedicated logging thread
    log_buffer: Mutex<Vec<OperationLog>>,
}

impl JsonFileLogger {
    pub fn new(log_path: PathBuf, max_entries: Option<usize>) -> Result<Self> {
        // Ensure the directory exists
        if let Some(parent) = log_path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }

        let mut logger = Self {
            log_path,
            max_entries,
            log_buffer: Mutex::new(Vec::new()),
        };
        logger.load_logs()?;
        Ok(logger)
    }

    fn load_logs(&mut self) -> Result<()> {
        if self.log_path.exists() {
            let content = fs::read_to_string(&self.log_path)?;
            if !content.trim().is_empty() {
                let logs: Vec<OperationLog> = serde_json::from_str(&content)?;
                *self.log_buffer.lock().unwrap() = logs;
            }
        }
        Ok(())
    }

    fn save_logs(&self) -> Result<()> {
        let logs = self.log_buffer.lock().unwrap();
        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true) // Overwrite existing content
            .open(&self.log_path)?;
        serde_json::to_writer_pretty(file, &*logs)?;
        Ok(())
    }
}

impl OperationLogger for JsonFileLogger {
    fn log_operation(&mut self, log: OperationLog) -> Result<()> {
        let mut buffer = self.log_buffer.lock().unwrap();
        buffer.push(log);
        if let Some(max) = self.max_entries {
            if buffer.len() > max {
                // Simple truncation: remove oldest entries
                buffer.drain(0..buffer.len() - max);
            }
        }
        self.save_logs()?;
        Ok(())
    }

    fn get_operation_history(&self, limit: Option<usize>) -> Result<Vec<OperationLog>> {
        let buffer = self.log_buffer.lock().unwrap();
        let history = if let Some(l) = limit {
            buffer.iter().rev().take(l).cloned().collect()
        } else {
            buffer.iter().rev().cloned().collect()
        };
        Ok(history)
    }

    fn get_operation_stats(&self) -> Result<HashMap<String, u64>> {
        let buffer = self.log_buffer.lock().unwrap();
        let mut stats = HashMap::new();
        for log in buffer.iter() {
            *stats.entry(log.operation_type.clone()).or_insert(0) += 1;
        }
        Ok(stats)
    }
}

pub fn create_logger(config: LoggerConfig) -> Result<Box<dyn OperationLogger + Send + Sync>> {
    match config.logger_type {
        LoggerType::Console => {
            // A simple console logger (not implemented as a struct here for brevity)
            // For a real app, you'd have a ConsoleLogger struct implementing OperationLogger
            println!("Console logger initialized. Logs will be printed to stdout.");
            Err(anyhow::anyhow!("Console logger not fully implemented as a trait object."))
        }
        LoggerType::Json => {
            let log_path = config.log_path.ok_or_else(|| anyhow::anyhow!("Log path required for JSON logger"))?;
            Ok(Box::new(JsonFileLogger::new(log_path, config.max_entries)?))
        }
    }
}
