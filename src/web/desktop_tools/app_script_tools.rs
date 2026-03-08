//! Execute scripts inside GPU-rendered applications (Blender bpy, etc.).
//!
//! These apps render their UI with OpenGL/Vulkan, so UI Automation doesn't work.
//! Instead, we invoke their built-in scripting engines via CLI.

use serde_json::Value;
use std::process::{Command, Stdio};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

use super::NativeToolResult;
use super::gpu_app_db;

/// Execute a script inside a GPU-rendered application.
///
/// Supported apps: blender (Python/bpy)
/// Args: { app, code, file (optional), background (default true) }
pub fn tool_execute_app_script(args: &Value) -> NativeToolResult {
    let app = match args.get("app").and_then(|v| v.as_str()) {
        Some(a) => a.to_lowercase(),
        None => return NativeToolResult::text_only(
            "Error: 'app' is required (e.g. \"blender\")".to_string(),
        ),
    };
    let code = match args.get("code").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return NativeToolResult::text_only(
            "Error: 'code' is required (script source code)".to_string(),
        ),
    };
    let file = args.get("file").and_then(|v| v.as_str());
    let background = args
        .get("background")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    match app.as_str() {
        "blender" => execute_blender_script(code, file, background),
        other => {
            // Check if it's a known GPU app without script support
            if let Some(info) = gpu_app_db::detect_gpu_app("", other) {
                if info.script_lang.is_some() {
                    NativeToolResult::text_only(format!(
                        "Error: {} scripting via execute_app_script is not yet implemented.\n\
                         Use execute_command to run it manually: {} {} <script>",
                        info.app_name,
                        other,
                        info.script_cli_flag.unwrap_or(""),
                    ))
                } else {
                    NativeToolResult::text_only(format!(
                        "Error: {} does not have built-in script support via this tool.",
                        info.app_name,
                    ))
                }
            } else {
                NativeToolResult::text_only(format!(
                    "Error: Unknown app '{}'. Supported: blender",
                    other,
                ))
            }
        }
    }
}

/// Execute a Python/bpy script inside Blender.
fn execute_blender_script(code: &str, blend_file: Option<&str>, background: bool) -> NativeToolResult {
    // Find blender executable
    let blender_exe = find_blender_exe();
    let exe = match &blender_exe {
        Some(path) => path.as_str(),
        None => return NativeToolResult::text_only(
            "Error: Blender not found. Install Blender or ensure it's on PATH / in Program Files.".to_string(),
        ),
    };

    // Write code to temp .py file
    let temp_dir = std::env::temp_dir();
    let script_path = temp_dir.join("llama_chat_blender_script.py");
    if let Err(e) = std::fs::write(&script_path, code) {
        return NativeToolResult::text_only(format!("Error writing temp script: {e}"));
    }

    // Build command: blender [--background] [file.blend] --python script.py
    let mut cmd = Command::new(exe);
    if background {
        cmd.arg("--background");
    }
    if let Some(f) = blend_file {
        cmd.arg(f);
    }
    cmd.arg("--python");
    cmd.arg(&script_path);

    // CRITICAL: stdin(null) to prevent pipe inheritance hang
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    // Hide the console window on Windows (blender.exe is a console app)
    #[cfg(target_os = "windows")]
    {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let output = match cmd.output() {
        Ok(o) => o,
        Err(e) => {
            let _ = std::fs::remove_file(&script_path);
            return NativeToolResult::text_only(format!("Error running Blender: {e}"));
        }
    };

    // Clean up temp file
    let _ = std::fs::remove_file(&script_path);

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Filter out Blender startup noise from stdout
    let filtered_stdout = filter_blender_output(&stdout);
    let filtered_stderr = filter_blender_stderr(&stderr);

    let status_msg = if output.status.success() {
        "Script completed successfully."
    } else {
        "Script failed."
    };

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
fn find_blender_exe() -> Option<String> {
    // 1. Check if "blender" is on PATH
    let mut check_cmd = Command::new("blender");
    check_cmd
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(target_os = "windows")]
    {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        check_cmd.creation_flags(CREATE_NO_WINDOW);
    }
    if check_cmd.status().is_ok() {
        return Some("blender".to_string());
    }

    // 2. Use the app discovery from window_tools
    super::window_tools::find_application_exe("blender")
}

/// Filter Blender startup noise from stdout.
fn filter_blender_output(raw: &str) -> String {
    raw.lines()
        .filter(|line| {
            let l = line.trim();
            // Skip common Blender startup lines
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

/// Filter Blender stderr noise (keep actual errors).
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
