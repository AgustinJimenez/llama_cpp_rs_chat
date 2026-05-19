//! Maya Python (mayapy) script execution.

use std::process::{Command, Stdio};

use super::super::NativeToolResult;
use super::common::{apply_no_window, run_command_with_timeout, SCRIPT_TIMEOUT};

/// Execute a Python script via Maya's `mayapy` standalone interpreter.
pub fn execute_maya_script(code: &str, _maya_file: Option<&str>) -> NativeToolResult {
    let maya_exe = find_maya_exe();
    let exe = match &maya_exe {
        Some(path) => path.as_str(),
        None => return super::super::tool_error(
            "execute_app_script", "Maya (mayapy) not found. Install Autodesk Maya or ensure mayapy is on PATH.",
        ),
    };

    let temp_dir = std::env::temp_dir();
    let script_path = temp_dir.join("llama_chat_maya_script.py");

    let wrapper = format!(
        "import sys\n\
         try:\n\
             import maya.standalone\n\
             maya.standalone.initialize()\n\
         except:\n\
             pass\n\n\
         {code}\n\n\
         try:\n\
             maya.standalone.uninitialize()\n\
         except:\n\
             pass\n"
    );

    if let Err(e) = std::fs::write(&script_path, &wrapper) {
        return super::super::tool_error("execute_app_script", format!("writing temp script: {e}"));
    }

    let mut cmd = Command::new(exe);
    cmd.arg(&script_path);
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    apply_no_window(&mut cmd);

    let output = match run_command_with_timeout(cmd, SCRIPT_TIMEOUT) {
        Ok(o) => o,
        Err(e) => {
            let _ = std::fs::remove_file(&script_path);
            return super::super::tool_error("execute_app_script", format!("running mayapy: {e}"));
        }
    };

    let _ = std::fs::remove_file(&script_path);

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let filtered_stdout = filter_maya_output(&stdout);
    let filtered_stderr = filter_maya_output(&stderr);

    let status_msg = if output.status.success() { "Script completed successfully." } else { "Script failed." };
    let mut result = status_msg.to_string();
    if !filtered_stdout.is_empty() {
        result.push_str(&format!("\n\nOutput:\n{filtered_stdout}"));
    }
    if !filtered_stderr.is_empty() {
        result.push_str(&format!("\n\nErrors:\n{filtered_stderr}"));
    }
    NativeToolResult::text_only(result)
}

pub fn find_maya_exe() -> Option<String> {
    let mut check_cmd = Command::new("mayapy");
    check_cmd.arg("--version").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    apply_no_window(&mut check_cmd);
    if check_cmd.status().map(|s| s.success()).unwrap_or(false) {
        return Some("mayapy".to_string());
    }

    let candidates = find_maya_installations();
    if let Some(latest) = candidates.last() {
        return Some(latest.clone());
    }
    super::super::window_tools::find_application_exe("mayapy")
}

fn find_maya_installations() -> Vec<String> {
    let mut results = Vec::new();

    #[cfg(target_os = "windows")]
    {
        let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".to_string());
        let root = std::path::PathBuf::from(&pf).join("Autodesk");
        if let Ok(entries) = std::fs::read_dir(&root) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_lowercase();
                if name.starts_with("maya") {
                    let mayapy = entry.path().join("bin").join("mayapy.exe");
                    if mayapy.is_file() {
                        results.push(mayapy.to_string_lossy().into_owned());
                    }
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        let root = "/Applications/Autodesk";
        if let Ok(entries) = std::fs::read_dir(root) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_lowercase();
                if name.starts_with("maya") {
                    let mayapy = entry.path().join("Maya.app").join("Contents").join("bin").join("mayapy");
                    if mayapy.is_file() {
                        results.push(mayapy.to_string_lossy().into_owned());
                    }
                }
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        let root = "/usr/autodesk";
        if let Ok(entries) = std::fs::read_dir(root) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_lowercase();
                if name.starts_with("maya") {
                    let mayapy = entry.path().join("bin").join("mayapy");
                    if mayapy.is_file() {
                        results.push(mayapy.to_string_lossy().into_owned());
                    }
                }
            }
        }
    }

    results.sort();
    results
}

fn filter_maya_output(raw: &str) -> String {
    raw.lines()
        .filter(|line| {
            let l = line.trim();
            !l.is_empty()
                && !l.starts_with("Autodesk Maya")
                && !l.starts_with("Maya ")
                && !l.starts_with("License Path:")
                && !l.starts_with("Plugin Manager:")
                && !l.starts_with("Loading plugin")
                && !l.starts_with("Initializing Maya")
                && !l.starts_with("Uninitializing Maya")
                && !l.starts_with("file: ")
                && !l.starts_with("Cut key default")
                && !l.contains("pymel")
                && !l.contains("Successfully initialized")
                && !l.contains("not found on MAYA_PLUG_IN_PATH")
        })
        .collect::<Vec<_>>()
        .join("\n")
}
