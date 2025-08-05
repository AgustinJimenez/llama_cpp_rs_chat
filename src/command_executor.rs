use crate::ai_operations::*;
use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};
use std::io::{BufRead, BufReader};
use uuid::Uuid;

pub struct SystemCommandExecutor {
    allowed_commands: HashSet<String>,
    dangerous_commands: HashSet<String>,
    max_execution_time: Duration,
    logger: Option<Box<dyn OperationLogger + Send + Sync>>,
}

impl SystemCommandExecutor {
    pub fn new() -> Self {
        let mut allowed_commands = HashSet::new();
        
        // Safe system commands
        allowed_commands.insert("ls".to_string());
        allowed_commands.insert("dir".to_string());
        allowed_commands.insert("cat".to_string());
        allowed_commands.insert("type".to_string());
        allowed_commands.insert("grep".to_string());
        allowed_commands.insert("findstr".to_string());
        allowed_commands.insert("find".to_string());
        allowed_commands.insert("where".to_string());
        allowed_commands.insert("ps".to_string());
        allowed_commands.insert("tasklist".to_string());
        allowed_commands.insert("curl".to_string());
        allowed_commands.insert("wget".to_string());
        allowed_commands.insert("git".to_string());
        allowed_commands.insert("npm".to_string());
        allowed_commands.insert("cargo".to_string());
        allowed_commands.insert("pip".to_string());
        allowed_commands.insert("python".to_string());
        allowed_commands.insert("node".to_string());
        allowed_commands.insert("rustc".to_string());
        allowed_commands.insert("gcc".to_string());
        allowed_commands.insert("make".to_string());
        allowed_commands.insert("cmake".to_string());
        allowed_commands.insert("echo".to_string());
        allowed_commands.insert("pwd".to_string());
        allowed_commands.insert("whoami".to_string());
        allowed_commands.insert("date".to_string());
        allowed_commands.insert("time".to_string());
        
        let mut dangerous_commands = HashSet::new();
        
        // Commands that require confirmation
        dangerous_commands.insert("rm".to_string());
        dangerous_commands.insert("del".to_string());
        dangerous_commands.insert("rmdir".to_string());
        dangerous_commands.insert("rd".to_string());
        dangerous_commands.insert("format".to_string());
        dangerous_commands.insert("fdisk".to_string());
        dangerous_commands.insert("dd".to_string());
        dangerous_commands.insert("shutdown".to_string());
        dangerous_commands.insert("reboot".to_string());
        dangerous_commands.insert("halt".to_string());
        dangerous_commands.insert("poweroff".to_string());
        dangerous_commands.insert("sudo".to_string());
        dangerous_commands.insert("su".to_string());
        dangerous_commands.insert("chmod".to_string());
        dangerous_commands.insert("chown".to_string());
        dangerous_commands.insert("mv".to_string());
        dangerous_commands.insert("move".to_string());
        dangerous_commands.insert("ren".to_string());
        dangerous_commands.insert("rename".to_string());

        Self {
            allowed_commands,
            dangerous_commands,
            max_execution_time: Duration::from_secs(300), // 5 minutes default
            logger: None,
        }
    }

    pub fn with_logger(mut self, logger: Box<dyn OperationLogger + Send + Sync>) -> Self {
        self.logger = Some(logger);
        self
    }

    pub fn add_allowed_command(&mut self, command: &str) {
        self.allowed_commands.insert(command.to_string());
    }

    pub fn remove_allowed_command(&mut self, command: &str) {
        self.allowed_commands.remove(command);
    }

    fn validate_command(&self, command: &str) -> Result<(), OperationError> {
        // Check for command injection patterns
        let dangerous_patterns = ["|", ";", "&", "$(", "`", "||", "&&"];
        for pattern in &dangerous_patterns {
            if command.contains(pattern) {
                return Err(OperationError::ValidationFailed(
                    format!("Command contains dangerous pattern: {}", pattern)
                ));
            }
        }

        // Check against allowed commands
        let base_command = command.split_whitespace().next().unwrap_or(command);
        if !self.allowed_commands.contains(base_command) {
            return Err(OperationError::CommandNotAllowed(base_command.to_string()));
        }

        Ok(())
    }

    fn execute_with_timeout(&self, mut cmd: Command, timeout: Duration) -> Result<CommandResponse> {
        let start_time = SystemTime::now();
        let command_id = Uuid::new_v4().to_string();

        cmd.stdout(Stdio::piped())
           .stderr(Stdio::piped());

        let mut child = cmd.spawn()
            .map_err(|e| anyhow::anyhow!("Failed to spawn command: {}", e))?;

        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();

        let stdout_reader = BufReader::new(stdout);
        let stderr_reader = BufReader::new(stderr);

        let mut output = String::new();
        let mut error = String::new();

        // Read stdout
        for line in stdout_reader.lines() {
            match line {
                Ok(line) => {
                    output.push_str(&line);
                    output.push('\n');
                }
                Err(_) => break,
            }
        }

        // Read stderr
        for line in stderr_reader.lines() {
            match line {
                Ok(line) => {
                    error.push_str(&line);
                    error.push('\n');
                }
                Err(_) => break,
            }
        }

        // Wait for process with timeout
        let exit_status = match child.try_wait() {
            Ok(Some(status)) => status,
            Ok(None) => {
                // Process is still running, wait with timeout
                std::thread::sleep(Duration::from_millis(100));
                match child.try_wait() {
                    Ok(Some(status)) => status,
                    _ => {
                        let _ = child.kill();
                        return Err(anyhow::anyhow!("Command timed out after {:?}", timeout));
                    }
                }
            }
            Err(e) => return Err(anyhow::anyhow!("Error waiting for command: {}", e)),
        };

        let execution_time = SystemTime::now()
            .duration_since(start_time)
            .unwrap_or_default()
            .as_millis() as u64;

        let exit_code = exit_status.code().unwrap_or(-1);
        let success = exit_status.success();

        Ok(CommandResponse {
            success,
            exit_code,
            output: output.trim().to_string(),
            error: error.trim().to_string(),
            execution_time_ms: execution_time,
            command_id,
        })
    }

    fn log_execution(&mut self, command: &str, response: &CommandResponse) {
        if let Some(ref mut logger) = self.logger {
            let log = OperationLog {
                timestamp: SystemTime::now(),
                operation_type: "COMMAND_EXEC".to_string(),
                status: if response.success { "SUCCESS" } else { "FAILED" }.to_string(),
                details: format!("{} (exit_code: {})", command, response.exit_code),
                user_id: None,
                execution_time_ms: Some(response.execution_time_ms),
            };
            let _ = logger.log_operation(log);
        }
    }
}

impl CommandExecutor for SystemCommandExecutor {
    fn execute(&self, request: CommandRequest) -> Result<CommandResponse> {
        let full_command = if request.args.is_empty() {
            request.command.clone()
        } else {
            format!("{} {}", request.command, request.args.join(" "))
        };

        // Validate command
        self.validate_command(&full_command)
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        let mut cmd = Command::new(&request.command);
        cmd.args(&request.args);

        // Set working directory
        if let Some(ref working_dir) = request.working_dir {
            cmd.current_dir(working_dir);
        }

        // Set environment variables
        for (key, value) in &request.environment {
            cmd.env(key, value);
        }

        let timeout = Duration::from_millis(
            request.timeout_ms.unwrap_or(self.max_execution_time.as_millis() as u64)
        );

        let mut response = self.execute_with_timeout(cmd, timeout)?;
        
        // Log the execution
        let mut executor = self.clone();
        executor.log_execution(&full_command, &response);

        // Add additional validation for dangerous commands
        if self.requires_confirmation(&request.command) && !request.require_confirmation {
            response.success = false;
            response.error = format!(
                "Command '{}' requires explicit confirmation. Use require_confirmation: true",
                request.command
            );
        }

        Ok(response)
    }

    fn is_safe(&self, command: &str) -> bool {
        let base_command = command.split_whitespace().next().unwrap_or(command);
        self.allowed_commands.contains(base_command) && 
        !self.dangerous_commands.contains(base_command)
    }

    fn requires_confirmation(&self, command: &str) -> bool {
        let base_command = command.split_whitespace().next().unwrap_or(command);
        self.dangerous_commands.contains(base_command)
    }

    fn get_command_help(&self, command: &str) -> Option<String> {
        match command {
            "ls" | "dir" => Some("List directory contents".to_string()),
            "cat" | "type" => Some("Display file contents".to_string()),
            "grep" | "findstr" => Some("Search text in files".to_string()),
            "git" => Some("Git version control operations".to_string()),
            "npm" => Some("Node.js package manager".to_string()),
            "cargo" => Some("Rust package manager".to_string()),
            "pip" => Some("Python package manager".to_string()),
            _ => None,
        }
    }
}

// Clone implementation for SystemCommandExecutor
impl Clone for SystemCommandExecutor {
    fn clone(&self) -> Self {
        Self {
            allowed_commands: self.allowed_commands.clone(),
            dangerous_commands: self.dangerous_commands.clone(),
            max_execution_time: self.max_execution_time,
            logger: None, // Logger is not cloneable
        }
    }
}