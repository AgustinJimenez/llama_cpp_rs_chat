/// Command execution module
/// 
/// This module handles the actual execution of commands extracted from AI responses.

use anyhow::Result;
use crate::ai_operations::{CommandRequest, CommandExecutor};
use crate::command_executor::SystemCommandExecutor;
use crate::llm_backend::ChatMessage;
use crate::command_detection::ExtractedCommand;
use std::collections::HashMap;
use std::path::PathBuf;

/// Execute a command extracted from AI response
/// 
/// Takes the extracted command, runs it through the system command executor,
/// and returns the formatted result for adding to conversation.
pub fn execute_command(extracted_command: ExtractedCommand) -> Result<ChatMessage> {
    println!("🚀 COMMAND_RUNNER: Starting command execution");
    println!("🚀 COMMAND_RUNNER: Command to execute: '{}'", extracted_command.command);
    
    // Create command executor
    let command_executor = SystemCommandExecutor::new();
    
    // Prepare command request
    let request = CommandRequest {
        command: extracted_command.command.clone(),
        args: vec![], // Full command is in the command field for better handling
        working_dir: Some(std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))),
        timeout_ms: Some(60000), // 60 seconds timeout
        environment: HashMap::new(),
    };

    println!("🚀 COMMAND_RUNNER: CommandRequest created:");
    println!("   Command: '{}'", request.command);
    println!("   Args: {:?}", request.args);
    println!("   Working dir: {:?}", request.working_dir);
    println!("   Timeout: {:?}ms", request.timeout_ms);
    
    println!("🚀 COMMAND_RUNNER: About to execute command...");
    
    // Execute the command
    match command_executor.execute(request) {
        Ok(result) => {
            println!("✅ COMMAND_RUNNER: Command executed successfully");
            println!("   Success: {}", result.success);
            println!("   Exit code: {}", result.exit_code);
            println!("   Output length: {} characters", result.output.len());
            println!("   Error length: {} characters", result.error.len());
            
            // Format the output for conversation
            let formatted_output = if result.success {
                if result.output.is_empty() {
                    "[Command executed successfully but produced no output]".to_string()
                } else {
                    // Truncate very long output to prevent issues
                    if result.output.len() > 10000 {
                        format!("{}... [truncated - {} chars total]", &result.output[..10000], result.output.len())
                    } else {
                        result.output
                    }
                }
            } else {
                format!("Error (exit code {}): {}", result.exit_code, 
                       if result.error.is_empty() { "Command failed with no error message" } else { &result.error })
            };

            println!("🚀 COMMAND_RUNNER: Formatted output ready ({} chars)", formatted_output.len());
            
            // Return as system message for conversation
            Ok(ChatMessage {
                role: "system".to_string(),
                content: format!("Command output:\n```\n{}\n```", formatted_output),
            })
        }
        Err(e) => {
            println!("❌ COMMAND_RUNNER: Command execution failed: {}", e);
            
            // Return error as system message
            Ok(ChatMessage {
                role: "system".to_string(),
                content: format!("Command execution failed: {}", e),
            })
        }
    }
}

/// Check if a command should trigger follow-up generation
/// 
/// Commands that read files or provide information should typically
/// be followed by AI analysis of the results.
pub fn should_generate_followup(command: &str) -> bool {
    let cmd_lower = command.to_lowercase();
    
    // Commands that typically need follow-up analysis
    let followup_commands = ["type", "cat", "dir", "ls", "find", "grep", "findstr"];
    
    let needs_followup = followup_commands.iter().any(|&cmd| cmd_lower.starts_with(cmd));
    
    println!("🔄 COMMAND_RUNNER: Command '{}' needs follow-up: {}", command, needs_followup);
    
    needs_followup
}