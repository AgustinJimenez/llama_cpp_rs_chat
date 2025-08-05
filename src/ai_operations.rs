use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandRequest {
    pub command: String,
    pub args: Vec<String>,
    pub working_dir: Option<PathBuf>,
    pub timeout_ms: Option<u64>,
    pub environment: HashMap<String, String>,
    pub require_confirmation: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandResponse {
    pub success: bool,
    pub exit_code: i32,
    pub output: String,
    pub error: String,
    pub execution_time_ms: u64,
    pub command_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FileOperation {
    Create { path: PathBuf, content: String },
    Read { path: PathBuf },
    Update { path: PathBuf, line: usize, content: String },
    Append { path: PathBuf, content: String },
    Delete { path: PathBuf },
    Move { from: PathBuf, to: PathBuf },
    Copy { from: PathBuf, to: PathBuf },
    Chmod { path: PathBuf, mode: u32 },
    CreateDir { path: PathBuf },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileOperationResult {
    pub success: bool,
    pub message: String,
    pub affected_files: Vec<PathBuf>,
    pub backup_location: Option<PathBuf>,
    pub operation_id: String,
}

#[derive(Debug, Clone)]
pub enum OperationError {
    CommandNotAllowed(String),
    FileNotFound(PathBuf),
    PermissionDenied(String),
    InsufficientSpace(u64),
    Timeout(Duration),
    ValidationFailed(String),
    BackupFailed(String),
    InvalidPath(String),
}

impl std::fmt::Display for OperationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OperationError::CommandNotAllowed(cmd) => write!(f, "Command not allowed: {}", cmd),
            OperationError::FileNotFound(path) => write!(f, "File not found: {}", path.display()),
            OperationError::PermissionDenied(msg) => write!(f, "Permission denied: {}", msg),
            OperationError::InsufficientSpace(needed) => write!(f, "Insufficient space: {} bytes needed", needed),
            OperationError::Timeout(duration) => write!(f, "Operation timed out after {:?}", duration),
            OperationError::ValidationFailed(msg) => write!(f, "Validation failed: {}", msg),
            OperationError::BackupFailed(msg) => write!(f, "Backup failed: {}", msg),
            OperationError::InvalidPath(path) => write!(f, "Invalid path: {}", path),
        }
    }
}

impl std::error::Error for OperationError {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationLog {
    pub timestamp: SystemTime,
    pub operation_type: String,
    pub status: String,
    pub details: String,
    pub user_id: Option<String>,
    pub execution_time_ms: Option<u64>,
}

pub trait CommandExecutor {
    fn execute(&self, request: CommandRequest) -> Result<CommandResponse>;
    fn is_safe(&self, command: &str) -> bool;
    fn requires_confirmation(&self, command: &str) -> bool;
    fn get_command_help(&self, command: &str) -> Option<String>;
}

pub trait FileSystemManager {
    fn execute_operation(&mut self, operation: FileOperation) -> Result<FileOperationResult>;
    fn create_backup(&self, path: &PathBuf) -> Result<PathBuf>;
    fn restore_backup(&self, backup_path: &PathBuf, original_path: &PathBuf) -> Result<()>;
    fn validate_path(&self, path: &PathBuf) -> Result<()>;
    fn get_disk_space(&self, path: &PathBuf) -> Result<u64>;
}

pub trait OperationLogger {
    fn log_operation(&mut self, log: OperationLog) -> Result<()>;
    fn get_operation_history(&self, limit: Option<usize>) -> Result<Vec<OperationLog>>;
    fn get_operation_stats(&self) -> Result<HashMap<String, u64>>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectTemplate {
    pub name: String,
    pub description: String,
    pub files: Vec<FileTemplate>,
    pub commands: Vec<String>,
    pub dependencies: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileTemplate {
    pub path: PathBuf,
    pub content: String,
    pub permissions: Option<u32>,
    pub is_executable: bool,
}

pub trait ProjectGenerator {
    fn generate_project(&self, template_name: &str, project_name: &str, target_dir: &PathBuf) -> Result<Vec<PathBuf>>;
    fn list_available_templates(&self) -> Vec<String>;
    fn get_template(&self, name: &str) -> Option<ProjectTemplate>;
}