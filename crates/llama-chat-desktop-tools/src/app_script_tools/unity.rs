//! Unity C# script execution (dotnet-script preferred, Unity batchmode fallback).

use std::process::{Command, Stdio};

use super::super::NativeToolResult;
use super::common::{apply_no_window, canonicalize_project_path, run_command_with_timeout, SCRIPT_TIMEOUT};

/// Execute a C# script via Unity batchmode or dotnet-script fallback.
pub fn execute_unity_script(code: &str, project_path: Option<&str>) -> NativeToolResult {
    if let Some(result) = try_dotnet_script(code) {
        return result;
    }

    let unity_exe = find_unity_exe();
    let exe = match &unity_exe {
        Some(path) => path.as_str(),
        None => return super::super::tool_error(
            "execute_app_script",
            "neither 'dotnet-script' nor Unity were found.\n\
             Install dotnet-script: dotnet tool install -g dotnet-script\n\
             Or install Unity Hub and ensure Unity is in its default location.",
        ),
    };

    let project = match project_path {
        Some(p) => match canonicalize_project_path(p) {
            Ok(canonical) => canonical,
            Err(e) => return super::super::tool_error("execute_app_script", e),
        },
        None => return super::super::tool_error(
            "execute_app_script",
            "Unity batchmode requires a 'file' parameter pointing to a Unity project directory.\n\
             Alternatively, install dotnet-script for ad-hoc C# execution: dotnet tool install -g dotnet-script",
        ),
    };

    let wrapper = format!(
        "using UnityEngine;\nusing UnityEditor;\n\n\
         public static class LlamaChatScript {{\n\
             public static void Run() {{\n\
                 {code}\n\
             }}\n\
         }}"
    );

    let temp_dir = std::env::temp_dir();
    let script_path = temp_dir.join("llama_chat_unity_script.cs");
    if let Err(e) = std::fs::write(&script_path, &wrapper) {
        return super::super::tool_error("execute_app_script", format!("writing temp script: {e}"));
    }

    let editor_dir = std::path::PathBuf::from(&project).join("Assets").join("Editor");
    let _ = std::fs::create_dir_all(&editor_dir);
    let project_script = editor_dir.join("LlamaChatScript.cs");
    if let Err(e) = std::fs::copy(&script_path, &project_script) {
        let _ = std::fs::remove_file(&script_path);
        return super::super::tool_error("execute_app_script", format!("copying script to Unity project: {e}"));
    }

    let mut cmd = Command::new(exe);
    cmd.args([
        "-batchmode", "-nographics", "-projectPath", &project,
        "-executeMethod", "LlamaChatScript.Run",
        "-logFile", "-", "-quit",
    ]);
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    apply_no_window(&mut cmd);

    let output = match run_command_with_timeout(cmd, SCRIPT_TIMEOUT) {
        Ok(o) => o,
        Err(e) => {
            let _ = std::fs::remove_file(&script_path);
            let _ = std::fs::remove_file(&project_script);
            return super::super::tool_error("execute_app_script", format!("running Unity: {e}"));
        }
    };

    let _ = std::fs::remove_file(&script_path);
    let _ = std::fs::remove_file(&project_script);

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let filtered_stdout = filter_unity_output(&stdout);
    let filtered_stderr = filter_unity_output(&stderr);

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

fn try_dotnet_script(code: &str) -> Option<NativeToolResult> {
    let mut check_cmd = Command::new("dotnet");
    check_cmd.args(["script", "--version"]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    apply_no_window(&mut check_cmd);
    match check_cmd.status() {
        Ok(status) if status.success() => {}
        _ => return None,
    }

    let temp_dir = std::env::temp_dir();
    let script_path = temp_dir.join("llama_chat_unity_script.csx");
    if let Err(e) = std::fs::write(&script_path, code) {
        return Some(super::super::tool_error("execute_app_script", format!("writing temp script: {e}")));
    }

    let mut cmd = Command::new("dotnet");
    cmd.arg("script").arg(&script_path);
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    apply_no_window(&mut cmd);

    let output = match run_command_with_timeout(cmd, SCRIPT_TIMEOUT) {
        Ok(o) => o,
        Err(e) => {
            let _ = std::fs::remove_file(&script_path);
            return Some(super::super::tool_error("execute_app_script", format!("running dotnet-script: {e}")));
        }
    };

    let _ = std::fs::remove_file(&script_path);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let status_msg = if output.status.success() {
        "Script completed successfully (via dotnet-script)."
    } else {
        "Script failed (via dotnet-script)."
    };
    let mut result = status_msg.to_string();
    if !stdout.trim().is_empty() {
        result.push_str(&format!("\n\nOutput:\n{}", stdout.trim()));
    }
    if !stderr.trim().is_empty() {
        result.push_str(&format!("\n\nErrors:\n{}", stderr.trim()));
    }
    Some(NativeToolResult::text_only(result))
}

pub fn find_unity_exe() -> Option<String> {
    let mut check_cmd = Command::new("Unity");
    check_cmd.arg("-version").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    apply_no_window(&mut check_cmd);
    if check_cmd.status().map(|s| s.success()).unwrap_or(false) {
        return Some("Unity".to_string());
    }

    let candidates = find_unity_hub_editors();
    if let Some(latest) = candidates.last() {
        return Some(latest.clone());
    }
    super::super::window_tools::find_application_exe("unity")
}

fn find_unity_hub_editors() -> Vec<String> {
    let mut results = Vec::new();

    #[cfg(target_os = "windows")]
    {
        let search_roots = [r"C:\Program Files\Unity\Hub\Editor", r"C:\Program Files\Unity Hub\Editor"];
        for root in &search_roots {
            if let Ok(entries) = std::fs::read_dir(root) {
                for entry in entries.flatten() {
                    let editor_exe = entry.path().join("Editor").join("Unity.exe");
                    if editor_exe.is_file() {
                        results.push(editor_exe.to_string_lossy().into_owned());
                    }
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        let root = "/Applications/Unity/Hub/Editor";
        if let Ok(entries) = std::fs::read_dir(root) {
            for entry in entries.flatten() {
                let exe = entry.path().join("Unity.app").join("Contents").join("MacOS").join("Unity");
                if exe.is_file() {
                    results.push(exe.to_string_lossy().into_owned());
                }
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        if let Ok(home) = std::env::var("HOME") {
            let root = format!("{home}/Unity/Hub/Editor");
            if let Ok(entries) = std::fs::read_dir(&root) {
                for entry in entries.flatten() {
                    let exe = entry.path().join("Editor").join("Unity");
                    if exe.is_file() {
                        results.push(exe.to_string_lossy().into_owned());
                    }
                }
            }
        }
    }

    results.sort();
    results
}

fn filter_unity_output(raw: &str) -> String {
    raw.lines()
        .filter(|line| {
            let l = line.trim();
            !l.is_empty()
                && !l.starts_with("Loading ...")
                && !l.starts_with("Refreshing native plugins")
                && !l.starts_with("Preloading ")
                && !l.starts_with("[Package Manager]")
                && !l.starts_with("Loaded Assets")
                && !l.starts_with("Native extension ")
                && !l.starts_with("Mono path")
                && !l.starts_with("- Completed reload")
                && !l.starts_with("Domain Reload Profiling")
                && !l.starts_with("  ReloadAssembly")
                && !l.starts_with("Launching external process")
                && !l.starts_with("LICENSE SYSTEM")
                && !l.starts_with("Successfully changed project path")
                && !l.starts_with("  Batch mode:")
                && !l.contains("[InitializeOnLoad]")
                && !l.contains("Unloading ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}
