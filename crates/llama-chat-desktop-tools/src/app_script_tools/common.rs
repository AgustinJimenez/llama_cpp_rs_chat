//! Shared utilities for app script execution.

use std::io::Read;
use std::process::Command;
use std::time::Duration;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

/// Default timeout for script execution (2 minutes).
pub const SCRIPT_TIMEOUT: Duration = Duration::from_secs(120);

/// Canonicalize and validate a user-provided file/project path.
pub fn canonicalize_project_path(raw: &str) -> Result<String, String> {
    let path = std::path::Path::new(raw);
    let canonical = path
        .canonicalize()
        .map_err(|e| format!("invalid path '{raw}': {e}"))?;
    Ok(canonical.to_string_lossy().into_owned())
}

/// Run a Command with a timeout. Kills the process if it exceeds the timeout.
pub fn run_command_with_timeout(
    mut cmd: Command,
    timeout: Duration,
) -> Result<std::process::Output, String> {
    let mut child = cmd.spawn().map_err(|e| format!("Failed to spawn: {e}"))?;
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                // Process exited, collect output
                let mut stdout = Vec::new();
                let mut stderr = Vec::new();
                if let Some(mut out) = child.stdout.take() {
                    let _ = out.read_to_end(&mut stdout);
                }
                if let Some(mut err) = child.stderr.take() {
                    let _ = err.read_to_end(&mut stderr);
                }
                return Ok(std::process::Output {
                    status,
                    stdout,
                    stderr,
                });
            }
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait(); // reap zombie
                    return Err(format!(
                        "Script timed out after {}s",
                        timeout.as_secs()
                    ));
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => {
                let _ = child.kill();
                return Err(format!("Error waiting for process: {e}"));
            }
        }
    }
}

/// Helper to apply CREATE_NO_WINDOW flag on Windows.
#[allow(unused_variables)]
pub fn apply_no_window(cmd: &mut Command) {
    #[cfg(target_os = "windows")]
    {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
}

#[cfg(test)]
mod tests {
    use super::canonicalize_project_path;

    #[test]
    fn test_canonicalize_valid_path() {
        let result = canonicalize_project_path(".");
        assert!(result.is_ok());
        let canonical = result.unwrap();
        assert!(!canonical.is_empty());
        #[cfg(windows)]
        assert!(canonical.contains(':') || canonical.starts_with("\\\\"));
    }

    #[test]
    fn test_canonicalize_nonexistent_path() {
        let result = canonicalize_project_path("/nonexistent/fake/path/project.blend");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("invalid path"));
        assert!(err.contains("/nonexistent/fake/path/project.blend"));
    }

    #[test]
    fn test_canonicalize_rejects_traversal() {
        let _result = canonicalize_project_path("../../../../../../etc/passwd");
        #[cfg(windows)]
        assert!(_result.is_err());
    }

    #[test]
    fn test_canonicalize_temp_dir() {
        let temp = std::env::temp_dir();
        let result = canonicalize_project_path(temp.to_str().unwrap());
        assert!(result.is_ok());
    }
}
