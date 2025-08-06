use crate::ai_operations::*;
use crate::command_executor::SystemCommandExecutor;
use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;
use regex::Regex;

pub struct AIOperationsManager {
    command_executor: SystemCommandExecutor,
}

impl AIOperationsManager {
    pub fn new() -> Result<Self> {
        let command_executor = SystemCommandExecutor::new();
        Ok(Self {
            command_executor,
        })
    }

    pub fn process_ai_request(&mut self, message: &str) -> Result<String> {
        if let Some(operation) = self.parse_operation_request(message)? {
            self.execute_ai_operation(operation)
        } else {
            Ok("No AI operation detected in the message.".to_string())
        }
    }

    fn parse_operation_request(&self, message: &str) -> Result<Option<AIOperation>> {
        let command_regex = Regex::new(r"!CMD!(.+?)!CMD!")?;
        if let Some(captures) = command_regex.captures(message) {
            let command_line = captures[1].to_string();
            let parts: Vec<&str> = command_line.split_whitespace().collect();
            if !parts.is_empty() {
                let operation = AIOperation::ExecuteCommand {
                    command: parts[0].to_string(),
                    args: parts[1..].iter().map(|s| s.to_string()).collect(),
                    working_dir: None,
                    timeout_ms: None,
                };
                return Ok(Some(operation));
            }
        }
        Ok(None)
    }

    fn execute_ai_operation(&mut self, operation: AIOperation) -> Result<String> {
        match operation {
            AIOperation::ExecuteCommand { command, args, working_dir, timeout_ms } => {
                let request = CommandRequest {
                    command: command.clone(),
                    args,
                    working_dir,
                    timeout_ms,
                    environment: HashMap::new(),
                };

                let response = self.command_executor.execute(request)?;
                if response.success {
                    Ok(format!("✅ Command executed successfully:\n{}", response.output))
                } else {
                    Ok(format!("❌ Command failed (exit code {}):\n{}", response.exit_code, response.error))
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum AIOperation {
    ExecuteCommand { command: String, args: Vec<String>, working_dir: Option<PathBuf>, timeout_ms: Option<u64> },
}
