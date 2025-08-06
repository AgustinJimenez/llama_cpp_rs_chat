use crate::ai_operations::*;
use anyhow::Result;
use std::process::{Command, Stdio};
use std::time::Duration;
use std::io::{BufRead, BufReader};
use uuid::Uuid;

pub struct SystemCommandExecutor;

impl SystemCommandExecutor {
    pub fn new() -> Self {
        Self
    }

    fn execute_with_timeout(&self, mut cmd: Command, timeout: Duration) -> Result<CommandResponse> {
        let command_id = Uuid::new_v4().to_string();
        let start_time = std::time::SystemTime::now();

        cmd.stdout(Stdio::piped())
           .stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| anyhow::anyhow!("Failed to spawn command: {}", e))?;

        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();

        let stdout_reader = BufReader::new(stdout);
        let stderr_reader = BufReader::new(stderr);

        let mut output = String::new();
        let mut error = String::new();

        let status = child.wait_with_output().map_err(|e| anyhow::anyhow!("Failed to wait for command: {}", e))?;

        output = String::from_utf8_lossy(&status.stdout).to_string();
        error = String::from_utf8_lossy(&status.stderr).to_string();

        let execution_time = start_time.elapsed().unwrap_or_default().as_millis() as u64;

        Ok(CommandResponse {
            success: status.status.success(),
            exit_code: status.status.code().unwrap_or(-1),
            output,
            error,
            execution_time_ms: execution_time,
            command_id,
        })
    }
}

impl CommandExecutor for SystemCommandExecutor {
    fn execute(&self, request: CommandRequest) -> Result<CommandResponse> {
        let mut cmd;
        if cfg!(target_os = "windows") {
            cmd = Command::new("cmd");
            let command_str = format!("/C {} {}", request.command, request.args.join(" "));
            cmd.arg(command_str);
        } else {
            cmd = Command::new(&request.command);
            cmd.args(&request.args);
        }

        if let Some(ref working_dir) = request.working_dir {
            cmd.current_dir(working_dir);
        }

        for (key, value) in &request.environment {
            cmd.env(key, value);
        }

        let timeout = Duration::from_millis(request.timeout_ms.unwrap_or(300_000));

        self.execute_with_timeout(cmd, timeout)
    }
}
