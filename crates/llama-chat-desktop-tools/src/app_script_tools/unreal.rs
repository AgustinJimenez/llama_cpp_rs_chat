//! Unreal Engine 5 Python script execution via UnrealEditor-Cmd.

use std::process::{Command, Stdio};

use super::super::NativeToolResult;
use super::common::{apply_no_window, canonicalize_project_path, run_command_with_timeout, SCRIPT_TIMEOUT};

/// Execute a Python script via Unreal Engine 5's `UnrealEditor-Cmd` executable.
pub fn execute_unreal_script(code: &str, project_path: Option<&str>) -> NativeToolResult {
    let ue_exe = find_unreal_exe();
    let exe = match &ue_exe {
        Some(path) => path.as_str(),
        None => return super::super::tool_error(
            "execute_app_script",
            "Unreal Engine (UnrealEditor-Cmd) not found.\n\
             Install UE5 via Epic Games Launcher or ensure UnrealEditor-Cmd is on PATH.",
        ),
    };

    let temp_dir = std::env::temp_dir();
    let script_path = temp_dir.join("llama_chat_unreal_script.py");
    if let Err(e) = std::fs::write(&script_path, code) {
        return super::super::tool_error("execute_app_script", format!("writing temp script: {e}"));
    }

    let script_path_str = script_path.to_string_lossy().to_string();

    let mut cmd = Command::new(exe);
    if let Some(project) = project_path {
        match canonicalize_project_path(project) {
            Ok(p) => { cmd.arg(p); }
            Err(e) => return super::super::tool_error("execute_app_script", e),
        }
    }
    cmd.arg(format!("-ExecutePythonScript=\"{script_path_str}\""));
    cmd.args(["-nullrhi", "-stdout", "-unattended", "-nosplash"]);
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    apply_no_window(&mut cmd);

    let output = match run_command_with_timeout(cmd, SCRIPT_TIMEOUT) {
        Ok(o) => o,
        Err(e) => {
            let _ = std::fs::remove_file(&script_path);
            return super::super::tool_error("execute_app_script", format!("running UnrealEditor-Cmd: {e}"));
        }
    };

    let _ = std::fs::remove_file(&script_path);

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let filtered_stdout = filter_unreal_output(&stdout);
    let filtered_stderr = filter_unreal_output(&stderr);

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

pub fn find_unreal_exe() -> Option<String> {
    let cmd_name = if cfg!(target_os = "windows") { "UnrealEditor-Cmd.exe" } else { "UnrealEditor-Cmd" };
    let mut check_cmd = Command::new(cmd_name);
    check_cmd.arg("-help").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    apply_no_window(&mut check_cmd);
    if check_cmd.status().map(|s| s.success()).unwrap_or(false) {
        return Some(cmd_name.to_string());
    }

    let candidates = find_unreal_installations();
    if let Some(latest) = candidates.last() {
        return Some(latest.clone());
    }
    super::super::window_tools::find_application_exe("UnrealEditor-Cmd")
}

fn find_unreal_installations() -> Vec<String> {
    let mut results = Vec::new();

    #[cfg(target_os = "windows")]
    {
        let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".to_string());
        let epic_root = std::path::PathBuf::from(&pf).join("Epic Games");
        if let Ok(entries) = std::fs::read_dir(&epic_root) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with("UE_5") || name.starts_with("UE_4") {
                    let exe = entry.path().join("Engine").join("Binaries").join("Win64").join("UnrealEditor-Cmd.exe");
                    if exe.is_file() {
                        results.push(exe.to_string_lossy().into_owned());
                    }
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        let root = "/Users/Shared/Epic Games";
        if let Ok(entries) = std::fs::read_dir(root) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with("UE_5") || name.starts_with("UE_4") {
                    let exe = entry.path().join("Engine").join("Binaries").join("Mac").join("UnrealEditor-Cmd");
                    if exe.is_file() {
                        results.push(exe.to_string_lossy().into_owned());
                    }
                }
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        if let Ok(home) = std::env::var("HOME") {
            let root = format!("{home}/UnrealEngine");
            let exe_path = std::path::PathBuf::from(&root)
                .join("Engine").join("Binaries").join("Linux").join("UnrealEditor-Cmd");
            if exe_path.is_file() {
                results.push(exe_path.to_string_lossy().into_owned());
            }
        }
    }

    results.sort();
    results
}

fn filter_unreal_output(raw: &str) -> String {
    raw.lines()
        .filter(|line| {
            let l = line.trim();
            if l.is_empty() { return false; }
            let noisy_prefixes = [
                "LogInit:", "LogConfig:", "LogPlatformFile:", "LogLinker:",
                "LogPackageName:", "LogAssetRegistry:", "LogShaderCompilers:",
                "LogMaterial:", "LogTexture:", "LogStreaming:", "LogAudio:",
            ];
            for prefix in &noisy_prefixes {
                if l.contains(prefix) { return false; }
            }
            if l.starts_with("Warning:") || l.starts_with("Display:") { return false; }
            if l.starts_with("Presizing for ") || l.starts_with("Loading ") { return false; }
            true
        })
        .collect::<Vec<_>>()
        .join("\n")
}
