use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandRequest {
    pub command: String,
    pub args: Vec<String>,
    pub working_dir: Option<PathBuf>,
    pub timeout_ms: Option<u64>,
    pub environment: HashMap<String, String>,
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

pub trait CommandExecutor {
    fn execute(&self, request: CommandRequest) -> Result<CommandResponse>;
}