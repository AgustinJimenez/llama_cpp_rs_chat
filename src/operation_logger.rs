use crate::ai_operations::*;
use anyhow::Result;
use serde_json;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::Mutex;

pub struct FileOperationLogger {
    log_file_path: PathBuf,
    file_handle: Mutex<File>,
}

impl FileOperationLogger {
    pub fn new(log_file_path: PathBuf) -> Result<Self> {
        // Create log directory if it doesn't exist
        if let Some(parent) = log_file_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let file_handle = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_file_path)?;

        Ok(Self {
            log_file_path,
            file_handle: Mutex::new(file_handle),
        })
    }

    fn format_log_entry(&self, log: &OperationLog) -> String {
        let timestamp = log.timestamp
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        let execution_time = log.execution_time_ms
            .map(|ms| format!(" ({}ms)", ms))
            .unwrap_or_default();

        format!(
            "{} [{}] [{}] {}{}",
            timestamp,
            log.operation_type,
            log.status,
            log.details,
            execution_time
        )
    }
}

impl OperationLogger for FileOperationLogger {
    fn log_operation(&mut self, log: OperationLog) -> Result<()> {
        let entry = self.format_log_entry(&log);
        
        if let Ok(mut file) = self.file_handle.lock() {
            writeln!(file, "{}", entry)?;
            file.flush()?;
        }
        
        Ok(())
    }

    fn get_operation_history(&self, limit: Option<usize>) -> Result<Vec<OperationLog>> {
        let file = File::open(&self.log_file_path)?;
        let reader = BufReader::new(file);
        let lines: Vec<String> = reader.lines().collect::<std::io::Result<Vec<_>>>()?;
        
        let start_index = if let Some(limit) = limit {
            lines.len().saturating_sub(limit)
        } else {
            0
        };

        let mut operations = Vec::new();
        for line in &lines[start_index..] {
            if let Ok(log) = self.parse_log_entry(line) {
                operations.push(log);
            }
        }

        Ok(operations)
    }

    fn get_operation_stats(&self) -> Result<HashMap<String, u64>> {
        let operations = self.get_operation_history(None)?;
        let mut stats = HashMap::new();

        for op in operations {
            let key = format!("{}_{}", op.operation_type, op.status);
            *stats.entry(key).or_insert(0) += 1;
            
            // Also count totals by operation type
            *stats.entry(op.operation_type).or_insert(0) += 1;
        }

        Ok(stats)
    }
}

impl FileOperationLogger {
    fn parse_log_entry(&self, line: &str) -> Result<OperationLog> {
        // Parse format: "timestamp [OPERATION_TYPE] [STATUS] details (execution_time)"
        let parts: Vec<&str> = line.splitn(4, ' ').collect();
        if parts.len() < 4 {
            return Err(anyhow::anyhow!("Invalid log entry format"));
        }

        let timestamp = std::time::UNIX_EPOCH + 
            std::time::Duration::from_secs(parts[0].parse()?);

        let operation_type = parts[1].trim_matches(['[', ']']).to_string();
        let status = parts[2].trim_matches(['[', ']']).to_string();
        let details_and_time = parts[3];

        // Extract execution time if present
        let (details, execution_time_ms) = if let Some(time_start) = details_and_time.rfind(" (") {
            if let Some(time_end) = details_and_time.rfind("ms)") {
                let details = details_and_time[..time_start].to_string();
                let time_str = &details_and_time[time_start + 2..time_end];
                let execution_time = time_str.parse().ok();
                (details, execution_time)
            } else {
                (details_and_time.to_string(), None)
            }
        } else {
            (details_and_time.to_string(), None)
        };

        Ok(OperationLog {
            timestamp,
            operation_type,
            status,
            details,
            user_id: None,
            execution_time_ms,
        })
    }
}

pub struct InMemoryOperationLogger {
    operations: Mutex<Vec<OperationLog>>,
    max_entries: usize,
}

impl InMemoryOperationLogger {
    pub fn new(max_entries: usize) -> Self {
        Self {
            operations: Mutex::new(Vec::new()),
            max_entries,
        }
    }
}

impl OperationLogger for InMemoryOperationLogger {
    fn log_operation(&mut self, log: OperationLog) -> Result<()> {
        if let Ok(mut operations) = self.operations.lock() {
            operations.push(log);
            
            // Keep only the last max_entries
            if operations.len() > self.max_entries {
                operations.remove(0);
            }
        }
        Ok(())
    }

    fn get_operation_history(&self, limit: Option<usize>) -> Result<Vec<OperationLog>> {
        if let Ok(operations) = self.operations.lock() {
            let start_index = if let Some(limit) = limit {
                operations.len().saturating_sub(limit)
            } else {
                0
            };
            Ok(operations[start_index..].to_vec())
        } else {
            Ok(Vec::new())
        }
    }

    fn get_operation_stats(&self) -> Result<HashMap<String, u64>> {
        let operations = self.get_operation_history(None)?;
        let mut stats = HashMap::new();

        for op in operations {
            let key = format!("{}_{}", op.operation_type, op.status);
            *stats.entry(key).or_insert(0) += 1;
            
            // Also count totals by operation type
            *stats.entry(op.operation_type).or_insert(0) += 1;
        }

        Ok(stats)
    }
}

pub struct JsonOperationLogger {
    log_file_path: PathBuf,
    operations: Mutex<Vec<OperationLog>>,
    max_entries: usize,
}

impl JsonOperationLogger {
    pub fn new(log_file_path: PathBuf, max_entries: usize) -> Result<Self> {
        // Create log directory if it doesn't exist
        if let Some(parent) = log_file_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Load existing operations from file
        let operations = if log_file_path.exists() {
            let content = std::fs::read_to_string(&log_file_path)?;
            if content.trim().is_empty() {
                Vec::new()
            } else {
                serde_json::from_str(&content).unwrap_or_default()
            }
        } else {
            Vec::new()
        };

        Ok(Self {
            log_file_path,
            operations: Mutex::new(operations),
            max_entries,
        })
    }

    fn save_to_file(&self) -> Result<()> {
        if let Ok(operations) = self.operations.lock() {
            let json = serde_json::to_string_pretty(&*operations)?;
            std::fs::write(&self.log_file_path, json)?;
        }
        Ok(())
    }
}

impl OperationLogger for JsonOperationLogger {
    fn log_operation(&mut self, log: OperationLog) -> Result<()> {
        if let Ok(mut operations) = self.operations.lock() {
            operations.push(log);
            
            // Keep only the last max_entries
            if operations.len() > self.max_entries {
                operations.remove(0);
            }
        }
        
        self.save_to_file()?;
        Ok(())
    }

    fn get_operation_history(&self, limit: Option<usize>) -> Result<Vec<OperationLog>> {
        if let Ok(operations) = self.operations.lock() {
            let start_index = if let Some(limit) = limit {
                operations.len().saturating_sub(limit)
            } else {
                0
            };
            Ok(operations[start_index..].to_vec())
        } else {
            Ok(Vec::new())
        }
    }

    fn get_operation_stats(&self) -> Result<HashMap<String, u64>> {
        let operations = self.get_operation_history(None)?;
        let mut stats = HashMap::new();

        for op in operations {
            let key = format!("{}_{}", op.operation_type, op.status);
            *stats.entry(key).or_insert(0) += 1;
            
            // Also count totals by operation type
            *stats.entry(op.operation_type).or_insert(0) += 1;
        }

        Ok(stats)
    }
}

// Helper function to create appropriate logger based on configuration
pub fn create_logger(config: LoggerConfig) -> Result<Box<dyn OperationLogger + Send + Sync>> {
    match config.logger_type {
        LoggerType::File => {
            let logger = FileOperationLogger::new(config.log_path.unwrap_or_else(|| {
                PathBuf::from("logs/operations.log")
            }))?;
            Ok(Box::new(logger))
        }
        LoggerType::Json => {
            let logger = JsonOperationLogger::new(
                config.log_path.unwrap_or_else(|| PathBuf::from("logs/operations.json")),
                config.max_entries.unwrap_or(10000)
            )?;
            Ok(Box::new(logger))
        }
        LoggerType::Memory => {
            let logger = InMemoryOperationLogger::new(
                config.max_entries.unwrap_or(1000)
            );
            Ok(Box::new(logger))
        }
    }
}

#[derive(Debug, Clone)]
pub enum LoggerType {
    File,
    Json,
    Memory,
}

#[derive(Debug, Clone)]
pub struct LoggerConfig {
    pub logger_type: LoggerType,
    pub log_path: Option<PathBuf>,
    pub max_entries: Option<usize>,
}