//! Blender Python/bpy script execution.

use std::process::{Command, Stdio};

use super::super::NativeToolResult;
use super::common::{apply_no_window, canonicalize_project_path, run_command_with_timeout, SCRIPT_TIMEOUT};

/// Execute a Python/bpy script inside Blender.
pub fn execute_blender_script(code: &str, blend_file: Option<&str>, background: bool) -> NativeToolResult {
    let blender_exe = find_blender_exe();
    let exe = match &blender_exe {
        Some(path) => path.as_str(),
        None => return super::super::tool_error(
            "execute_app_script", "Blender not found. Install Blender or ensure it's on PATH / in Program Files.",
        ),
    };

    let temp_dir = std::env::temp_dir();
    let script_path = temp_dir.join("llama_chat_blender_script.py");
    if let Err(e) = std::fs::write(&script_path, code) {
        return super::super::tool_error("execute_app_script", format!("writing temp script: {e}"));
    }

    let mut cmd = Command::new(exe);
    if background {
        cmd.arg("--background");
    }
    if let Some(f) = blend_file {
        match canonicalize_project_path(f) {
            Ok(p) => { cmd.arg(p); }
            Err(e) => return super::super::tool_error("execute_app_script", e),
        }
    }
    cmd.arg("--python");
    cmd.arg(&script_path);

    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    apply_no_window(&mut cmd);

    let output = match run_command_with_timeout(cmd, SCRIPT_TIMEOUT) {
        Ok(o) => o,
        Err(e) => {
            let _ = std::fs::remove_file(&script_path);
            return super::super::tool_error("execute_app_script", format!("running Blender: {e}"));
        }
    };

    let _ = std::fs::remove_file(&script_path);

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let filtered_stdout = filter_blender_output(&stdout);
    let filtered_stderr = filter_blender_stderr(&stderr);

    let status_msg = if output.status.success() { "Script completed successfully." } else { "Script failed." };
    let mut result = format!("{status_msg}");
    if !filtered_stdout.is_empty() {
        result.push_str(&format!("\n\nOutput:\n{filtered_stdout}"));
    }
    if !filtered_stderr.is_empty() {
        result.push_str(&format!("\n\nErrors:\n{filtered_stderr}"));
    }

    NativeToolResult::text_only(result)
}

/// Find the Blender executable.
pub fn find_blender_exe() -> Option<String> {
    let mut check_cmd = Command::new("blender");
    check_cmd
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    apply_no_window(&mut check_cmd);
    if check_cmd.status().is_ok() {
        return Some("blender".to_string());
    }
    super::super::window_tools::find_application_exe("blender")
}

fn filter_blender_output(raw: &str) -> String {
    raw.lines()
        .filter(|line| {
            let l = line.trim();
            !l.starts_with("Blender ")
                && !l.starts_with("Read prefs:")
                && !l.starts_with("found bundled")
                && !l.starts_with("Read blend:")
                && !l.starts_with("Info: ")
                && !l.starts_with("Fra:")
                && !l.is_empty()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn filter_blender_stderr(raw: &str) -> String {
    raw.lines()
        .filter(|line| {
            let l = line.trim();
            !l.is_empty()
                && !l.starts_with("AL lib:")
                && !l.starts_with("ALSA ")
                && !l.starts_with("Color management:")
                && !l.contains("libdecor")
                && !l.contains("WARNING **: ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}
