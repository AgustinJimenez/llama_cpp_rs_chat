use crate::ai_operations::*;
use anyhow::Result;
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use uuid::Uuid;

pub struct SystemFileManager {
    backup_dir: PathBuf,
    max_file_size: u64,
    allowed_extensions: Option<Vec<String>>,
    logger: Option<Box<dyn OperationLogger + Send + Sync>>,
}

impl SystemFileManager {
    pub fn new(backup_dir: PathBuf) -> Result<Self> {
        // Create backup directory if it doesn't exist
        if !backup_dir.exists() {
            fs::create_dir_all(&backup_dir)?;
        }

        Ok(Self {
            backup_dir,
            max_file_size: 100 * 1024 * 1024, // 100MB default
            allowed_extensions: None,
            logger: None,
        })
    }

    pub fn with_logger(mut self, logger: Box<dyn OperationLogger + Send + Sync>) -> Self {
        self.logger = Some(logger);
        self
    }

    pub fn with_max_file_size(mut self, size: u64) -> Self {
        self.max_file_size = size;
        self
    }

    pub fn with_allowed_extensions(mut self, extensions: Vec<String>) -> Self {
        self.allowed_extensions = Some(extensions);
        self
    }

    fn validate_extension(&self, path: &PathBuf) -> Result<(), OperationError> {
        if let Some(ref allowed) = self.allowed_extensions {
            if let Some(extension) = path.extension() {
                let ext = extension.to_string_lossy().to_lowercase();
                if !allowed.contains(&ext) {
                    return Err(OperationError::ValidationFailed(
                        format!("File extension '{}' not allowed", ext)
                    ));
                }
            } else if !allowed.contains(&"".to_string()) {
                return Err(OperationError::ValidationFailed(
                    "Files without extension not allowed".to_string()
                ));
            }
        }
        Ok(())
    }

    fn validate_file_size(&self, content: &str) -> Result<(), OperationError> {
        if content.len() as u64 > self.max_file_size {
            return Err(OperationError::ValidationFailed(
                format!("File size {} exceeds maximum {}", content.len(), self.max_file_size)
            ));
        }
        Ok(())
    }

    fn generate_backup_path(&self, original_path: &PathBuf) -> PathBuf {
        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        let filename = original_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        
        let backup_filename = format!("{}_{}.backup", filename, timestamp);
        self.backup_dir.join(backup_filename)
    }

    fn log_operation(&mut self, operation_type: &str, status: &str, details: &str, execution_time_ms: Option<u64>) {
        if let Some(ref mut logger) = self.logger {
            let log = OperationLog {
                timestamp: SystemTime::now(),
                operation_type: operation_type.to_string(),
                status: status.to_string(),
                details: details.to_string(),
                user_id: None,
                execution_time_ms,
            };
            let _ = logger.log_operation(log);
        }
    }

    fn create_file_impl(&mut self, path: &PathBuf, content: &str) -> Result<FileOperationResult> {
        let start_time = SystemTime::now();
        let operation_id = Uuid::new_v4().to_string();

        // Validate
        self.validate_path(path)?;
        self.validate_extension(path).map_err(|e| anyhow::anyhow!("{}", e))?;
        self.validate_file_size(content).map_err(|e| anyhow::anyhow!("{}", e))?;

        // Create parent directories if needed
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }

        // Create backup if file exists
        let backup_location = if path.exists() {
            Some(self.create_backup(path)?)
        } else {
            None
        };

        // Write file
        let mut file = File::create(path)?;
        file.write_all(content.as_bytes())?;
        file.sync_all()?;

        let execution_time = SystemTime::now()
            .duration_since(start_time)
            .unwrap_or_default()
            .as_millis() as u64;

        self.log_operation(
            "FILE_CREATE",
            "SUCCESS",
            &format!("{} ({} bytes)", path.display(), content.len()),
            Some(execution_time),
        );

        Ok(FileOperationResult {
            success: true,
            message: format!("File created successfully: {}", path.display()),
            affected_files: vec![path.clone()],
            backup_location,
            operation_id,
        })
    }

    fn read_file_impl(&self, path: &PathBuf) -> Result<String> {
        self.validate_path(path)?;
        
        if !path.exists() {
            return Err(anyhow::anyhow!("{}", OperationError::FileNotFound(path.clone())));
        }

        let metadata = fs::metadata(path)?;
        if metadata.len() > self.max_file_size {
            return Err(anyhow::anyhow!(
                "File too large: {} bytes (max: {})",
                metadata.len(),
                self.max_file_size
            ));
        }

        let content = fs::read_to_string(path)?;
        Ok(content)
    }

    fn update_file_impl(&mut self, path: &PathBuf, line: usize, new_content: &str) -> Result<FileOperationResult> {
        let start_time = SystemTime::now();
        let operation_id = Uuid::new_v4().to_string();

        // Validate
        self.validate_path(path)?;
        
        if !path.exists() {
            return Err(anyhow::anyhow!("{}", OperationError::FileNotFound(path.clone())));
        }

        // Create backup
        let backup_location = Some(self.create_backup(path)?);

        // Read current content
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let mut lines: Vec<String> = reader.lines().collect::<std::io::Result<Vec<_>>>()?;

        // Update the specified line
        if line > 0 && line <= lines.len() {
            lines[line - 1] = new_content.to_string();
        } else if line == lines.len() + 1 {
            // Append new line
            lines.push(new_content.to_string());
        } else {
            return Err(anyhow::anyhow!("Line number {} out of range (1-{})", line, lines.len()));
        }

        // Write updated content
        let updated_content = lines.join("\n");
        self.validate_file_size(&updated_content).map_err(|e| anyhow::anyhow!("{}", e))?;

        let mut file = File::create(path)?;
        file.write_all(updated_content.as_bytes())?;
        file.sync_all()?;

        let execution_time = SystemTime::now()
            .duration_since(start_time)
            .unwrap_or_default()
            .as_millis() as u64;

        self.log_operation(
            "FILE_UPDATE",
            "SUCCESS",
            &format!("{} (line {})", path.display(), line),
            Some(execution_time),
        );

        Ok(FileOperationResult {
            success: true,
            message: format!("File updated successfully: {} (line {})", path.display(), line),
            affected_files: vec![path.clone()],
            backup_location,
            operation_id,
        })
    }

    fn append_file_impl(&mut self, path: &PathBuf, content: &str) -> Result<FileOperationResult> {
        let start_time = SystemTime::now();
        let operation_id = Uuid::new_v4().to_string();

        self.validate_path(path)?;

        // Create backup if file exists
        let backup_location = if path.exists() {
            Some(self.create_backup(path)?)
        } else {
            None
        };

        // Append content
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        
        file.write_all(content.as_bytes())?;
        file.sync_all()?;

        let execution_time = SystemTime::now()
            .duration_since(start_time)
            .unwrap_or_default()
            .as_millis() as u64;

        self.log_operation(
            "FILE_APPEND",
            "SUCCESS",
            &format!("{} ({} bytes)", path.display(), content.len()),
            Some(execution_time),
        );

        Ok(FileOperationResult {
            success: true,
            message: format!("Content appended to file: {}", path.display()),
            affected_files: vec![path.clone()],
            backup_location,
            operation_id,
        })
    }

    fn delete_file_impl(&mut self, path: &PathBuf) -> Result<FileOperationResult> {
        let start_time = SystemTime::now();
        let operation_id = Uuid::new_v4().to_string();

        self.validate_path(path)?;

        if !path.exists() {
            return Err(anyhow::anyhow!("{}", OperationError::FileNotFound(path.clone())));
        }

        // Create backup before deletion
        let backup_location = Some(self.create_backup(path)?);

        // Delete file or directory
        if path.is_dir() {
            fs::remove_dir_all(path)?;
        } else {
            fs::remove_file(path)?;
        }

        let execution_time = SystemTime::now()
            .duration_since(start_time)
            .unwrap_or_default()
            .as_millis() as u64;

        self.log_operation(
            "FILE_DELETE",
            "SUCCESS",
            &format!("{}", path.display()),
            Some(execution_time),
        );

        Ok(FileOperationResult {
            success: true,
            message: format!("File deleted: {}", path.display()),
            affected_files: vec![path.clone()],
            backup_location,
            operation_id,
        })
    }

    fn copy_file_impl(&mut self, from: &PathBuf, to: &PathBuf) -> Result<FileOperationResult> {
        let start_time = SystemTime::now();
        let operation_id = Uuid::new_v4().to_string();

        self.validate_path(from)?;
        self.validate_path(to)?;

        if !from.exists() {
            return Err(anyhow::anyhow!("{}", OperationError::FileNotFound(from.clone())));
        }

        // Create backup if destination exists
        let backup_location = if to.exists() {
            Some(self.create_backup(to)?)
        } else {
            None
        };

        // Create parent directories for destination
        if let Some(parent) = to.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }

        // Copy file or directory
        if from.is_dir() {
            copy_dir_all(from, to)?;
        } else {
            fs::copy(from, to)?;
        }

        let execution_time = SystemTime::now()
            .duration_since(start_time)
            .unwrap_or_default()
            .as_millis() as u64;

        self.log_operation(
            "FILE_COPY",
            "SUCCESS",
            &format!("{} -> {}", from.display(), to.display()),
            Some(execution_time),
        );

        Ok(FileOperationResult {
            success: true,
            message: format!("File copied: {} -> {}", from.display(), to.display()),
            affected_files: vec![from.clone(), to.clone()],
            backup_location,
            operation_id,
        })
    }
}

impl FileSystemManager for SystemFileManager {
    fn execute_operation(&mut self, operation: FileOperation) -> Result<FileOperationResult> {
        match operation {
            FileOperation::Create { path, content } => {
                self.create_file_impl(&path, &content)
            }
            FileOperation::Read { path } => {
                let content = self.read_file_impl(&path)?;
                Ok(FileOperationResult {
                    success: true,
                    message: format!("File read successfully: {} ({} bytes)", path.display(), content.len()),
                    affected_files: vec![path],
                    backup_location: None,
                    operation_id: Uuid::new_v4().to_string(),
                })
            }
            FileOperation::Update { path, line, content } => {
                self.update_file_impl(&path, line, &content)
            }
            FileOperation::Append { path, content } => {
                self.append_file_impl(&path, &content)
            }
            FileOperation::Delete { path } => {
                self.delete_file_impl(&path)
            }
            FileOperation::Copy { from, to } => {
                self.copy_file_impl(&from, &to)
            }
            FileOperation::Move { from, to } => {
                self.validate_path(&from)?;
                self.validate_path(&to)?;
                
                if !from.exists() {
                    return Err(anyhow::anyhow!("{}", OperationError::FileNotFound(from)));
                }

                // Create backup if destination exists
                let backup_location = if to.exists() {
                    Some(self.create_backup(&to)?)
                } else {
                    None
                };

                fs::rename(&from, &to)?;

                self.log_operation(
                    "FILE_MOVE",
                    "SUCCESS",
                    &format!("{} -> {}", from.display(), to.display()),
                    None,
                );

                Ok(FileOperationResult {
                    success: true,
                    message: format!("File moved: {} -> {}", from.display(), to.display()),
                    affected_files: vec![from, to.clone()],
                    backup_location,
                    operation_id: Uuid::new_v4().to_string(),
                })
            }
            FileOperation::CreateDir { path } => {
                self.validate_path(&path)?;
                fs::create_dir_all(&path)?;

                self.log_operation(
                    "DIR_CREATE",
                    "SUCCESS",
                    &format!("{}", path.display()),
                    None,
                );

                Ok(FileOperationResult {
                    success: true,
                    message: format!("Directory created: {}", path.display()),
                    affected_files: vec![path],
                    backup_location: None,
                    operation_id: Uuid::new_v4().to_string(),
                })
            }
            FileOperation::Chmod { path, mode: _ } => {
                // Note: chmod is Unix-specific, Windows uses different permission model
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let permissions = std::fs::Permissions::from_mode(mode);
                    std::fs::set_permissions(&path, permissions)?;
                }
                
                Ok(FileOperationResult {
                    success: true,
                    message: format!("Permissions updated: {}", path.display()),
                    affected_files: vec![path],
                    backup_location: None,
                    operation_id: Uuid::new_v4().to_string(),
                })
            }
        }
    }

    fn create_backup(&self, path: &PathBuf) -> Result<PathBuf> {
        if !path.exists() {
            return Err(anyhow::anyhow!("{}", OperationError::FileNotFound(path.clone())));
        }

        let backup_path = self.generate_backup_path(path);
        
        if path.is_dir() {
            copy_dir_all(path, &backup_path)?;
        } else {
            fs::copy(path, &backup_path)?;
        }

        Ok(backup_path)
    }

    fn restore_backup(&self, backup_path: &PathBuf, original_path: &PathBuf) -> Result<()> {
        if !backup_path.exists() {
            return Err(anyhow::anyhow!("{}", OperationError::FileNotFound(backup_path.clone())));
        }

        if backup_path.is_dir() {
            if original_path.exists() {
                fs::remove_dir_all(original_path)?;
            }
            copy_dir_all(backup_path, original_path)?;
        } else {
            fs::copy(backup_path, original_path)?;
        }

        Ok(())
    }

    fn validate_path(&self, path: &PathBuf) -> Result<()> {
        let path_str = path.to_string_lossy();
        
        // Check for path traversal
        if path_str.contains("..") {
            return Err(anyhow::anyhow!("{}", OperationError::InvalidPath(
                "Path traversal not allowed".to_string()
            )));
        }

        // Check for system directories (basic protection)
        let dangerous_paths = ["/etc", "/bin", "/sbin", "/usr/bin", "/usr/sbin", "C:\\Windows", "C:\\System32"];
        for dangerous in &dangerous_paths {
            if path_str.starts_with(dangerous) {
                return Err(anyhow::anyhow!("{}", OperationError::PermissionDenied(
                    format!("Access to system directory not allowed: {}", dangerous)
                )));
            }
        }

        Ok(())
    }

    fn get_disk_space(&self, path: &PathBuf) -> Result<u64> {
        // This is a simplified implementation
        // In production, you'd use platform-specific APIs
        let metadata = fs::metadata(path.parent().unwrap_or(path))?;
        Ok(metadata.len()) // This is not actual disk space, just a placeholder
    }
}

// Helper function to copy directories recursively
fn copy_dir_all(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> std::io::Result<()> {
    fs::create_dir_all(&dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir_all(entry.path(), dst.as_ref().join(entry.file_name()))?;
        } else {
            fs::copy(entry.path(), dst.as_ref().join(entry.file_name()))?;
        }
    }
    Ok(())
}