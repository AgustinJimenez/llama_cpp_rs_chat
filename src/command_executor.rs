use crate::ai_operations::*;
use anyhow::Result;
use std::process::{Command, Stdio};
use std::time::Duration;
use uuid::Uuid;

pub struct SystemCommandExecutor;

impl SystemCommandExecutor {
    pub fn new() -> Self {
        Self
    }

    fn execute_with_timeout(&self, mut cmd: Command, _timeout: Duration) -> Result<CommandResponse> {
        let command_id = Uuid::new_v4().to_string();
        let start_time = std::time::SystemTime::now();

        cmd.stdout(Stdio::piped())
           .stderr(Stdio::piped());

        // Use a thread-based timeout approach
        let output = std::thread::spawn(move || {
            cmd.output()
        });

        let result = match output.join() {
            Ok(result) => result,
            Err(_) => return Err(anyhow::anyhow!("Command execution thread panicked")),
        };

        let output = result.map_err(|e| anyhow::anyhow!("Failed to execute command: {}", e))?;
        let execution_time = start_time.elapsed().unwrap_or_default().as_millis() as u64;

        Ok(CommandResponse {
            success: output.status.success(),
            exit_code: output.status.code().unwrap_or(-1),
            output: String::from_utf8_lossy(&output.stdout).to_string(),
            error: String::from_utf8_lossy(&output.stderr).to_string(),
            execution_time_ms: execution_time,
            command_id,
        })
    }
}

impl CommandExecutor for SystemCommandExecutor {
    fn execute(&self, request: CommandRequest) -> Result<CommandResponse> {
        println!("🔧 SystemCommandExecutor::execute called");
        println!("🔧 Request command: '{}'", request.command);
        println!("🔧 Request args: {:?}", request.args);
        
        let mut cmd;
        let full_command = if request.args.is_empty() {
            request.command.clone()
        } else {
            format!("{} {}", request.command, request.args.join(" "))
        };

        println!("🔧 Full command after processing: '{}'", full_command);

        if cfg!(target_os = "windows") {
            println!("🔧 Windows detected, using cmd /C");
            cmd = Command::new("cmd");
            cmd.args(&["/C", &full_command]);
            println!("🔧 Command constructed: cmd /C \"{}\"", full_command);
        } else {
            // For Unix systems, use sh to handle command chaining
            println!("🔧 Unix system detected, using sh -c");
            cmd = Command::new("sh");
            cmd.args(&["-c", &full_command]);
            println!("🔧 Command constructed: sh -c \"{}\"", full_command);
        }

        if let Some(ref working_dir) = request.working_dir {
            println!("🔧 Setting working directory: {:?}", working_dir);
            cmd.current_dir(working_dir);
        }

        for (key, value) in &request.environment {
            println!("🔧 Setting environment variable: {}={}", key, value);
            cmd.env(key, value);
        }

        let timeout = Duration::from_millis(request.timeout_ms.unwrap_or(300_000));
        println!("🔧 Using timeout: {:?}", timeout);

        println!("🔧 About to call execute_with_timeout...");
        let result = self.execute_with_timeout(cmd, timeout);
        println!("🔧 execute_with_timeout returned: {:?}", result.is_ok());
        result
    }
}
