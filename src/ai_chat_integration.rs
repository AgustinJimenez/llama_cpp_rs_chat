use crate::ai_operations::*;
use crate::command_executor::SystemCommandExecutor;
use crate::file_manager::SystemFileManager;
use crate::operation_logger::*;
use crate::project_templates::ProjectTemplateManager;
use crate::llm_backend::*;
use anyhow::Result;
use std::collections::HashMap;
use std::io::{self, Write};
use std::path::PathBuf;
use regex::Regex;

pub struct AIOperationsManager {
    command_executor: SystemCommandExecutor,
    file_manager: SystemFileManager,
    operation_logger: Box<dyn OperationLogger + Send + Sync>,
    project_templates: ProjectTemplateManager,
    confirmation_required: bool,
}

impl AIOperationsManager {
    pub fn new() -> Result<Self> {
        let backup_dir = PathBuf::from("backups");
        let log_config = LoggerConfig {
            logger_type: LoggerType::Json,
            log_path: Some(PathBuf::from("logs/ai_operations.json")),
            max_entries: Some(5000),
        };

        let logger = create_logger(log_config)?;
        let file_manager = SystemFileManager::new(backup_dir)?
            .with_max_file_size(50 * 1024 * 1024) // 50MB limit
            .with_allowed_extensions(vec![
                "txt".to_string(), "md".to_string(), "rs".to_string(),
                "py".to_string(), "js".to_string(), "ts".to_string(),
                "json".to_string(), "yaml".to_string(), "yml".to_string(),
                "toml".to_string(), "cfg".to_string(), "ini".to_string(),
                "html".to_string(), "css".to_string(), "xml".to_string(),
                "sh".to_string(), "bat".to_string(), "ps1".to_string(),
                "".to_string(), // Allow files without extension
            ]);

        let command_executor = SystemCommandExecutor::new();
        let project_templates = ProjectTemplateManager::new();

        Ok(Self {
            command_executor,
            file_manager,
            operation_logger: logger,
            project_templates,
            confirmation_required: true,
        })
    }

    pub fn set_confirmation_required(&mut self, required: bool) {
        self.confirmation_required = required;
    }

    pub fn process_ai_request(&mut self, message: &str) -> Result<String> {
        // Parse AI requests for special operations
        if let Some(operation) = self.parse_operation_request(message)? {
            self.execute_ai_operation(operation)
        } else {
            Ok("No AI operation detected in the message.".to_string())
        }
    }

    fn parse_operation_request(&self, message: &str) -> Result<Option<AIOperation>> {
        // Define patterns for different operations
        let patterns = [
            // File operations
            (r"/create-file\s+(\S+)\s+(.+)", AIOperationType::CreateFile),
            (r"/modify-file\s+(\S+)\s+(\d+)\s+(.+)", AIOperationType::ModifyFile),
            (r"/append-file\s+(\S+)\s+(.+)", AIOperationType::AppendFile),
            (r"/delete-file\s+(\S+)", AIOperationType::DeleteFile),
            (r"/read-file\s+(\S+)", AIOperationType::ReadFile),
            (r"/copy-file\s+(\S+)\s+(\S+)", AIOperationType::CopyFile),
            (r"/move-file\s+(\S+)\s+(\S+)", AIOperationType::MoveFile),
            (r"/create-dir\s+(\S+)", AIOperationType::CreateDirectory),
            (r"/list-dir\s*(\S*)", AIOperationType::ListDirectory),
            (r"/ls\s*(\S*)", AIOperationType::ListDirectory),
            
            // Command operations
            (r"/execute\s+(.+)", AIOperationType::ExecuteCommand),
            (r"/run\s+(.+)", AIOperationType::ExecuteCommand),
            
            // Project operations
            (r"/create-project\s+(\S+)\s+(\S+)", AIOperationType::CreateProject),
            (r"/list-templates", AIOperationType::ListTemplates),
            
            // System operations
            (r"/backup\s+(\S+)", AIOperationType::BackupFile),
            (r"/restore\s+(\S+)\s+(\S+)", AIOperationType::RestoreBackup),
            (r"/status", AIOperationType::ShowStatus),
            (r"/help", AIOperationType::ShowHelp),
        ];

        for (pattern, op_type) in &patterns {
            let regex = Regex::new(pattern)?;
            if let Some(captures) = regex.captures(message) {
                let operation = match op_type {
                    AIOperationType::CreateFile => {
                        if captures.len() >= 3 {
                            AIOperation::CreateFile {
                                path: PathBuf::from(&captures[1]),
                                content: captures[2].to_string(),
                            }
                        } else { continue; }
                    }
                    AIOperationType::ModifyFile => {
                        if captures.len() >= 4 {
                            AIOperation::ModifyFile {
                                path: PathBuf::from(&captures[1]),
                                line: captures[2].parse().unwrap_or(1),
                                content: captures[3].to_string(),
                            }
                        } else { continue; }
                    }
                    AIOperationType::AppendFile => {
                        if captures.len() >= 3 {
                            AIOperation::AppendFile {
                                path: PathBuf::from(&captures[1]),
                                content: captures[2].to_string(),
                            }
                        } else { continue; }
                    }
                    AIOperationType::DeleteFile => {
                        AIOperation::DeleteFile {
                            path: PathBuf::from(&captures[1]),
                        }
                    }
                    AIOperationType::ReadFile => {
                        AIOperation::ReadFile {
                            path: PathBuf::from(&captures[1]),
                        }
                    }
                    AIOperationType::CopyFile => {
                        if captures.len() >= 3 {
                            AIOperation::CopyFile {
                                from: PathBuf::from(&captures[1]),
                                to: PathBuf::from(&captures[2]),
                            }
                        } else { continue; }
                    }
                    AIOperationType::MoveFile => {
                        if captures.len() >= 3 {
                            AIOperation::MoveFile {
                                from: PathBuf::from(&captures[1]),
                                to: PathBuf::from(&captures[2]),
                            }
                        } else { continue; }
                    }
                    AIOperationType::CreateDirectory => {
                        AIOperation::CreateDirectory {
                            path: PathBuf::from(&captures[1]),
                        }
                    }
                    AIOperationType::ListDirectory => {
                        let path = if captures.len() > 1 && !captures[1].is_empty() {
                            PathBuf::from(&captures[1])
                        } else {
                            PathBuf::from(".")
                        };
                        AIOperation::ListDirectory { path }
                    }
                    AIOperationType::ExecuteCommand => {
                        let command_line = captures[1].to_string();
                        let parts: Vec<&str> = command_line.split_whitespace().collect();
                        if !parts.is_empty() {
                            AIOperation::ExecuteCommand {
                                command: parts[0].to_string(),
                                args: parts[1..].iter().map(|s| s.to_string()).collect(),
                                working_dir: None,
                                timeout_ms: None,
                            }
                        } else { continue; }
                    }
                    AIOperationType::CreateProject => {
                        if captures.len() >= 3 {
                            AIOperation::CreateProject {
                                project_type: captures[1].to_string(),
                                name: captures[2].to_string(),
                                path: PathBuf::from(&captures[2]),
                            }
                        } else { continue; }
                    }
                    AIOperationType::BackupFile => {
                        AIOperation::BackupFile {
                            path: PathBuf::from(&captures[1]),
                        }
                    }
                    AIOperationType::RestoreBackup => {
                        if captures.len() >= 3 {
                            AIOperation::RestoreBackup {
                                backup_path: PathBuf::from(&captures[1]),
                                original_path: PathBuf::from(&captures[2]),
                            }
                        } else { continue; }
                    }
                    AIOperationType::ShowStatus => AIOperation::ShowStatus,
                    AIOperationType::ShowHelp => AIOperation::ShowHelp,
                    AIOperationType::ListTemplates => AIOperation::ListTemplates,
                };
                return Ok(Some(operation));
            }
        }

        // Check for natural language requests
        if let Some(operation) = self.parse_natural_language_request(message)? {
            return Ok(Some(operation));
        }

        Ok(None)
    }

    fn parse_natural_language_request(&self, message: &str) -> Result<Option<AIOperation>> {
        let message_lower = message.to_lowercase();
        
        // Simple natural language patterns
        if message_lower.contains("create") && message_lower.contains("file") {
            // Try to extract file path and content from natural language
            // This is a simplified implementation
            return Ok(None); // For now, require explicit commands
        }
        
        if message_lower.contains("run") || message_lower.contains("execute") {
            // Try to extract command from natural language
            return Ok(None); // For now, require explicit commands
        }

        Ok(None)
    }

    fn execute_ai_operation(&mut self, operation: AIOperation) -> Result<String> {
        // Check if confirmation is required for dangerous operations
        if self.confirmation_required && self.is_dangerous_operation(&operation) {
            if !self.get_user_confirmation(&operation)? {
                return Ok("Operation cancelled by user.".to_string());
            }
        }

        match operation {
            AIOperation::CreateFile { path, content } => {
                let result = self.file_manager.execute_operation(
                    FileOperation::Create { path: path.clone(), content }
                )?;
                Ok(format!("✅ {}", result.message))
            }
            AIOperation::ModifyFile { path, line, content } => {
                let result = self.file_manager.execute_operation(
                    FileOperation::Update { path: path.clone(), line, content }
                )?;
                Ok(format!("✅ {}", result.message))
            }
            AIOperation::AppendFile { path, content } => {
                let result = self.file_manager.execute_operation(
                    FileOperation::Append { path: path.clone(), content }
                )?;
                Ok(format!("✅ {}", result.message))
            }
            AIOperation::DeleteFile { path } => {
                let result = self.file_manager.execute_operation(
                    FileOperation::Delete { path: path.clone() }
                )?;
                Ok(format!("✅ {}", result.message))
            }
            AIOperation::ReadFile { path } => {
                let result = self.file_manager.execute_operation(
                    FileOperation::Read { path: path.clone() }
                )?;
                Ok(format!("📄 File content read successfully from {}", path.display()))
            }
            AIOperation::CopyFile { from, to } => {
                let result = self.file_manager.execute_operation(
                    FileOperation::Copy { from: from.clone(), to: to.clone() }
                )?;
                Ok(format!("✅ {}", result.message))
            }
            AIOperation::MoveFile { from, to } => {
                let result = self.file_manager.execute_operation(
                    FileOperation::Move { from: from.clone(), to: to.clone() }
                )?;
                Ok(format!("✅ {}", result.message))
            }
            AIOperation::CreateDirectory { path } => {
                let result = self.file_manager.execute_operation(
                    FileOperation::CreateDir { path: path.clone() }
                )?;
                Ok(format!("✅ {}", result.message))
            }
            AIOperation::ListDirectory { path } => {
                match std::fs::read_dir(&path) {
                    Ok(entries) => {
                        let mut files = Vec::new();
                        let mut dirs = Vec::new();
                        
                        for entry in entries {
                            if let Ok(entry) = entry {
                                let path = entry.path();
                                let name = entry.file_name().to_string_lossy().to_string();
                                if path.is_dir() {
                                    dirs.push(format!("📁 {}/", name));
                                } else {
                                    let size = entry.metadata()
                                        .map(|m| {
                                            let size = m.len();
                                            if size < 1024 {
                                                format!(" ({} B)", size)
                                            } else if size < 1024 * 1024 {
                                                format!(" ({:.1} KB)", size as f64 / 1024.0)
                                            } else {
                                                format!(" ({:.1} MB)", size as f64 / (1024.0 * 1024.0))
                                            }
                                        })
                                        .unwrap_or_default();
                                    files.push(format!("📄 {}{}", name, size));
                                }
                            }
                        }
                        
                        dirs.sort();
                        files.sort();
                        
                        let mut result = format!("📂 Directory listing for {}:\n", path.display());
                        if dirs.is_empty() && files.is_empty() {
                            result.push_str("  (empty directory)");
                        } else {
                            for dir in dirs {
                                result.push_str(&format!("  {}\n", dir));
                            }
                            for file in files {
                                result.push_str(&format!("  {}\n", file));
                            }
                        }
                        Ok(result)
                    }
                    Err(e) => {
                        Ok(format!("❌ Failed to list directory {}: {}", path.display(), e))
                    }
                }
            }
            AIOperation::ExecuteCommand { command, args, working_dir, timeout_ms } => {
                let request = CommandRequest {
                    command: command.clone(),
                    args,
                    working_dir,
                    timeout_ms,
                    environment: HashMap::new(),
                    require_confirmation: false,
                };
                
                let response = self.command_executor.execute(request)?;
                if response.success {
                    Ok(format!("✅ Command executed successfully:\n{}", response.output))
                } else {
                    Ok(format!("❌ Command failed (exit code {}):\n{}", response.exit_code, response.error))
                }
            }
            AIOperation::CreateProject { project_type, name, path } => {
                match self.project_templates.generate_project(&project_type, &name, &path) {
                    Ok(created_files) => {
                        Ok(format!("✅ Project created successfully!\n   Template: {}\n   Location: {}\n   Files created: {}", 
                                  project_type, path.display(), created_files.len()))
                    }
                    Err(e) => {
                        Ok(format!("❌ Failed to create project: {}", e))
                    }
                }
            }
            AIOperation::BackupFile { path } => {
                let backup_path = self.file_manager.create_backup(&path)?;
                Ok(format!("✅ Backup created: {} -> {}", path.display(), backup_path.display()))
            }
            AIOperation::RestoreBackup { backup_path, original_path } => {
                self.file_manager.restore_backup(&backup_path, &original_path)?;
                Ok(format!("✅ Backup restored: {} -> {}", backup_path.display(), original_path.display()))
            }
            AIOperation::ShowStatus => {
                let stats = self.operation_logger.get_operation_stats()?;
                let mut status = String::from("📊 AI Operations Status:\n");
                for (key, count) in stats {
                    status.push_str(&format!("  {}: {}\n", key, count));
                }
                Ok(status)
            }
            AIOperation::ShowHelp => {
                Ok(self.get_help_text())
            }
            AIOperation::ListTemplates => {
                let templates = self.project_templates.list_available_templates();
                let mut response = String::from("📋 Available Project Templates:\n");
                for template_name in templates {
                    if let Some(template) = self.project_templates.get_template(&template_name) {
                        response.push_str(&format!("  • {} - {}\n", template_name, template.description));
                    }
                }
                response.push_str("\nUsage: /create-project <template> <project_name>");
                Ok(response)
            }
        }
    }

    fn is_dangerous_operation(&self, operation: &AIOperation) -> bool {
        matches!(operation, 
            AIOperation::DeleteFile { .. } |
            AIOperation::ExecuteCommand { .. } |
            AIOperation::MoveFile { .. }
        )
    }

    fn get_user_confirmation(&self, operation: &AIOperation) -> Result<bool> {
        println!("\n⚠️ Confirmation required for potentially dangerous operation:");
        println!("   {:?}", operation);
        print!("Do you want to proceed? (y/N): ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        
        Ok(input.trim().to_lowercase() == "y" || input.trim().to_lowercase() == "yes")
    }

    fn get_help_text(&self) -> String {
        r#"🤖 AI Operations Help

File Operations:
  /create-file <path> <content>     - Create a new file
  /modify-file <path> <line> <text> - Modify line in file  
  /append-file <path> <content>     - Append to file
  /delete-file <path>               - Delete file (requires confirmation)
  /read-file <path>                 - Read file contents
  /copy-file <from> <to>            - Copy file
  /move-file <from> <to>            - Move file (requires confirmation)
  /create-dir <path>                - Create directory
  /list-dir [path]                  - List directory contents (default: current)
  /ls [path]                        - Alias for /list-dir

Command Operations:
  /execute <command>                - Execute system command (requires confirmation)
  /run <command>                    - Alias for /execute

Project Operations:
  /create-project <type> <name>     - Generate project from template
  /list-templates                   - List available project templates

Backup Operations:
  /backup <path>                    - Create backup of file/directory
  /restore <backup> <original>      - Restore from backup

System Operations:
  /status                           - Show operation statistics
  /help                             - Show this help

Examples:
  /list-dir                         - List current directory
  /read-file TODO.md                - Read a specific file
  /create-file src/hello.rs "fn main() { println!(\"Hello!\"); }"
  /create-project rust-cli my-app
  /list-templates
  /execute ls -la
  /backup important.txt
  /create-dir src/modules

Safety Features:
- Automatic backups before modifications
- Command validation and whitelisting  
- User confirmation for dangerous operations
- Comprehensive operation logging
- Path traversal protection
"#.to_string()
    }
}

#[derive(Debug, Clone)]
pub enum AIOperation {
    CreateFile { path: PathBuf, content: String },
    ModifyFile { path: PathBuf, line: usize, content: String },
    AppendFile { path: PathBuf, content: String },
    DeleteFile { path: PathBuf },
    ReadFile { path: PathBuf },
    CopyFile { from: PathBuf, to: PathBuf },
    MoveFile { from: PathBuf, to: PathBuf },
    CreateDirectory { path: PathBuf },
    ListDirectory { path: PathBuf },
    ExecuteCommand { command: String, args: Vec<String>, working_dir: Option<PathBuf>, timeout_ms: Option<u64> },
    CreateProject { project_type: String, name: String, path: PathBuf },
    BackupFile { path: PathBuf },
    RestoreBackup { backup_path: PathBuf, original_path: PathBuf },
    ShowStatus,
    ShowHelp,
    ListTemplates,
}

#[derive(Debug, Clone)]
enum AIOperationType {
    CreateFile,
    ModifyFile,
    AppendFile,
    DeleteFile,
    ReadFile,
    CopyFile,
    MoveFile,
    CreateDirectory,
    ListDirectory,
    ExecuteCommand,
    CreateProject,
    BackupFile,
    RestoreBackup,
    ShowStatus,
    ShowHelp,
    ListTemplates,
}