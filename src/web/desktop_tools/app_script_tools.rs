//! Execute scripts inside GPU-rendered applications (Blender bpy, Unity C#, Maya Python, Godot GDScript, Unreal Engine 5 Python).
//!
//! These apps render their UI with OpenGL/Vulkan, so UI Automation doesn't work.
//! Instead, we invoke their built-in scripting engines via CLI.

use serde_json::Value;
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::Duration;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

use super::NativeToolResult;
use super::gpu_app_db;

/// Default timeout for script execution (2 minutes).
const SCRIPT_TIMEOUT: Duration = Duration::from_secs(120);

/// Run a Command with a timeout. Kills the process if it exceeds the timeout.
fn run_command_with_timeout(
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

/// Execute a script inside a GPU-rendered application.
///
/// Supported apps: blender (Python/bpy), unity (C#), maya (Python), godot (GDScript), unreal/ue/ue5 (Python)
/// Args: { app, code, file (optional), background (default true) }
pub fn tool_execute_app_script(args: &Value) -> NativeToolResult {
    let app = match args.get("app").and_then(|v| v.as_str()) {
        Some(a) => a.to_lowercase(),
        None => return super::tool_error(
            "execute_app_script", "'app' is required (e.g. \"blender\")",
        ),
    };
    let code = match args.get("code").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return super::tool_error(
            "execute_app_script", "'code' is required (script source code)",
        ),
    };
    let file = args.get("file").and_then(|v| v.as_str());
    let background = args
        .get("background")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    match app.as_str() {
        "blender" => execute_blender_script(code, file, background),
        "unity" => execute_unity_script(code, file),
        "maya" => execute_maya_script(code, file),
        "godot" => execute_godot_script(code, file),
        "unreal" | "ue" | "ue5" => execute_unreal_script(code, file),
        other => {
            // Check if it's a known GPU app without script support
            if let Some(info) = gpu_app_db::detect_gpu_app("", other) {
                if info.script_lang.is_some() {
                    super::tool_error("execute_app_script", format!(
                        "{} scripting via execute_app_script is not yet implemented.\n\
                         Use execute_command to run it manually: {} {} <script>",
                        info.app_name,
                        other,
                        info.script_cli_flag.unwrap_or(""),
                    ))
                } else {
                    super::tool_error("execute_app_script", format!(
                        "{} does not have built-in script support via this tool.",
                        info.app_name,
                    ))
                }
            } else {
                super::tool_error("execute_app_script", format!(
                    "unknown app '{}'. Supported: blender, unity, maya, godot, unreal",
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
        None => return super::tool_error(
            "execute_app_script", "Blender not found. Install Blender or ensure it's on PATH / in Program Files.",
        ),
    };

    // Write code to temp .py file
    let temp_dir = std::env::temp_dir();
    let script_path = temp_dir.join("llama_chat_blender_script.py");
    if let Err(e) = std::fs::write(&script_path, code) {
        return super::tool_error("execute_app_script", format!("writing temp script: {e}"));
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

    let output = match run_command_with_timeout(cmd, SCRIPT_TIMEOUT) {
        Ok(o) => o,
        Err(e) => {
            let _ = std::fs::remove_file(&script_path);
            return super::tool_error("execute_app_script", format!("running Blender: {e}"));
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

// ---------------------------------------------------------------------------
// Unity scripting support
// ---------------------------------------------------------------------------

/// Execute a C# script via Unity batchmode or dotnet-script fallback.
///
/// Unity's `-executeMethod` requires a compiled C# class inside a Unity project,
/// which is impractical for ad-hoc scripts. We prefer `dotnet-script` (dotnet tool)
/// which can run `.csx` C# scripts directly. If that's unavailable, we fall back
/// to Unity batchmode with a temporary Editor script.
fn execute_unity_script(code: &str, project_path: Option<&str>) -> NativeToolResult {
    // Try dotnet-script first (much simpler for ad-hoc scripts)
    if let Some(result) = try_dotnet_script(code) {
        return result;
    }

    // Fall back to Unity batchmode
    let unity_exe = find_unity_exe();
    let exe = match &unity_exe {
        Some(path) => path.as_str(),
        None => return super::tool_error(
            "execute_app_script",
            "neither 'dotnet-script' nor Unity were found.\n\
             Install dotnet-script: dotnet tool install -g dotnet-script\n\
             Or install Unity Hub and ensure Unity is in its default location.",
        ),
    };

    let project = match project_path {
        Some(p) => p.to_string(),
        None => return super::tool_error(
            "execute_app_script",
            "Unity batchmode requires a 'file' parameter pointing to a Unity project directory.\n\
             Alternatively, install dotnet-script for ad-hoc C# execution: dotnet tool install -g dotnet-script",
        ),
    };

    // Write C# code as an Editor script so -executeMethod can find it.
    // We wrap the user's code in a static class with a static method.
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
        return super::tool_error("execute_app_script", format!("writing temp script: {e}"));
    }

    // Copy script into the project's Assets/Editor folder so Unity can compile it
    let editor_dir = std::path::PathBuf::from(&project)
        .join("Assets")
        .join("Editor");
    let _ = std::fs::create_dir_all(&editor_dir);
    let project_script = editor_dir.join("LlamaChatScript.cs");
    if let Err(e) = std::fs::copy(&script_path, &project_script) {
        let _ = std::fs::remove_file(&script_path);
        return super::tool_error("execute_app_script", format!(
            "copying script to Unity project: {e}"
        ));
    }

    let mut cmd = Command::new(exe);
    cmd.args([
        "-batchmode",
        "-nographics",
        "-projectPath",
        &project,
        "-executeMethod",
        "LlamaChatScript.Run",
        "-logFile",
        "-", // Log to stdout
        "-quit",
    ]);

    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    #[cfg(target_os = "windows")]
    {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let output = match run_command_with_timeout(cmd, SCRIPT_TIMEOUT) {
        Ok(o) => o,
        Err(e) => {
            let _ = std::fs::remove_file(&script_path);
            let _ = std::fs::remove_file(&project_script);
            return super::tool_error("execute_app_script", format!("running Unity: {e}"));
        }
    };

    // Clean up
    let _ = std::fs::remove_file(&script_path);
    let _ = std::fs::remove_file(&project_script);

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let filtered_stdout = filter_unity_output(&stdout);
    let filtered_stderr = filter_unity_output(&stderr);

    let status_msg = if output.status.success() {
        "Script completed successfully."
    } else {
        "Script failed."
    };

    let mut result = status_msg.to_string();
    if !filtered_stdout.is_empty() {
        result.push_str(&format!("\n\nOutput:\n{filtered_stdout}"));
    }
    if !filtered_stderr.is_empty() {
        result.push_str(&format!("\n\nErrors:\n{filtered_stderr}"));
    }

    NativeToolResult::text_only(result)
}

/// Try running C# code via dotnet-script (dotnet tool).
/// Returns None if dotnet-script is not available.
fn try_dotnet_script(code: &str) -> Option<NativeToolResult> {
    // Check if dotnet-script is available
    let mut check_cmd = Command::new("dotnet");
    check_cmd
        .args(["script", "--version"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(target_os = "windows")]
    {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        check_cmd.creation_flags(CREATE_NO_WINDOW);
    }
    match check_cmd.status() {
        Ok(status) if status.success() => {}
        _ => return None,
    }

    let temp_dir = std::env::temp_dir();
    let script_path = temp_dir.join("llama_chat_unity_script.csx");
    if let Err(e) = std::fs::write(&script_path, code) {
        return Some(super::tool_error("execute_app_script", format!(
            "writing temp script: {e}"
        )));
    }

    let mut cmd = Command::new("dotnet");
    cmd.arg("script");
    cmd.arg(&script_path);

    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    #[cfg(target_os = "windows")]
    {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let output = match run_command_with_timeout(cmd, SCRIPT_TIMEOUT) {
        Ok(o) => o,
        Err(e) => {
            let _ = std::fs::remove_file(&script_path);
            return Some(super::tool_error("execute_app_script", format!(
                "running dotnet-script: {e}"
            )));
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

/// Find the Unity Editor executable.
fn find_unity_exe() -> Option<String> {
    // 1. Check if "Unity" is on PATH (unlikely but possible)
    let mut check_cmd = Command::new("Unity");
    check_cmd
        .arg("-version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(target_os = "windows")]
    {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        check_cmd.creation_flags(CREATE_NO_WINDOW);
    }
    if check_cmd.status().map(|s| s.success()).unwrap_or(false) {
        return Some("Unity".to_string());
    }

    // 2. Search common Unity Hub installation paths (pick the latest version)
    let candidates = find_unity_hub_editors();
    if let Some(latest) = candidates.last() {
        return Some(latest.clone());
    }

    // 3. Generic app discovery fallback
    super::window_tools::find_application_exe("unity")
}

/// Search Unity Hub editor installations, returning paths sorted by version.
fn find_unity_hub_editors() -> Vec<String> {
    let mut results = Vec::new();

    #[cfg(target_os = "windows")]
    {
        let search_roots = [
            r"C:\Program Files\Unity\Hub\Editor",
            r"C:\Program Files\Unity Hub\Editor",
        ];
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
                let exe = entry
                    .path()
                    .join("Unity.app")
                    .join("Contents")
                    .join("MacOS")
                    .join("Unity");
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

/// Filter Unity output noise.
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

// ---------------------------------------------------------------------------
// Maya scripting support
// ---------------------------------------------------------------------------

/// Execute a Python script via Maya's `mayapy` standalone interpreter.
fn execute_maya_script(code: &str, _maya_file: Option<&str>) -> NativeToolResult {
    let maya_exe = find_maya_exe();
    let exe = match &maya_exe {
        Some(path) => path.as_str(),
        None => return super::tool_error(
            "execute_app_script", "Maya (mayapy) not found. Install Autodesk Maya or ensure mayapy is on PATH.",
        ),
    };

    // Write code to temp .py file
    let temp_dir = std::env::temp_dir();
    let script_path = temp_dir.join("llama_chat_maya_script.py");

    // Wrap user code: initialize Maya standalone if not already, then run user code
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
        return super::tool_error("execute_app_script", format!("writing temp script: {e}"));
    }

    let mut cmd = Command::new(exe);
    cmd.arg(&script_path);

    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    #[cfg(target_os = "windows")]
    {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let output = match run_command_with_timeout(cmd, SCRIPT_TIMEOUT) {
        Ok(o) => o,
        Err(e) => {
            let _ = std::fs::remove_file(&script_path);
            return super::tool_error("execute_app_script", format!("running mayapy: {e}"));
        }
    };

    let _ = std::fs::remove_file(&script_path);

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let filtered_stdout = filter_maya_output(&stdout);
    let filtered_stderr = filter_maya_output(&stderr);

    let status_msg = if output.status.success() {
        "Script completed successfully."
    } else {
        "Script failed."
    };

    let mut result = status_msg.to_string();
    if !filtered_stdout.is_empty() {
        result.push_str(&format!("\n\nOutput:\n{filtered_stdout}"));
    }
    if !filtered_stderr.is_empty() {
        result.push_str(&format!("\n\nErrors:\n{filtered_stderr}"));
    }

    NativeToolResult::text_only(result)
}

/// Find Maya's `mayapy` standalone Python interpreter.
fn find_maya_exe() -> Option<String> {
    // 1. Check if "mayapy" is on PATH
    let mut check_cmd = Command::new("mayapy");
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
    if check_cmd.status().map(|s| s.success()).unwrap_or(false) {
        return Some("mayapy".to_string());
    }

    // 2. Search common Autodesk Maya installation paths (pick the latest version)
    let candidates = find_maya_installations();
    if let Some(latest) = candidates.last() {
        return Some(latest.clone());
    }

    // 3. Generic app discovery fallback
    super::window_tools::find_application_exe("mayapy")
}

/// Search for Maya installations, returning mayapy paths sorted by version.
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
                    let mayapy = entry
                        .path()
                        .join("Maya.app")
                        .join("Contents")
                        .join("bin")
                        .join("mayapy");
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

/// Filter Maya output noise.
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

// ---------------------------------------------------------------------------
// Godot scripting support
// ---------------------------------------------------------------------------

/// Execute a GDScript via Godot's `--headless --script` mode.
///
/// The user's script is wrapped in a minimal `SceneTree` extension that
/// runs the code in `_init()` and then quits.
fn execute_godot_script(code: &str, project_path: Option<&str>) -> NativeToolResult {
    let godot_exe = find_godot_exe();
    let exe = match &godot_exe {
        Some(path) => path.as_str(),
        None => return super::tool_error(
            "execute_app_script", "Godot not found. Install Godot or ensure it's on PATH.",
        ),
    };

    // Wrap user code in a SceneTree script that runs and quits.
    // Indent each line of user code by one tab for the _init() body.
    let indented_code = code
        .lines()
        .map(|line| {
            if line.trim().is_empty() {
                String::new()
            } else {
                format!("\t{line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    let wrapper = format!(
        "extends SceneTree\n\n\
         func _init():\n\
         {indented_code}\n\
         \tquit()\n"
    );

    // Write to temp .gd file
    let temp_dir = std::env::temp_dir();
    let script_path = temp_dir.join("llama_chat_godot_script.gd");
    if let Err(e) = std::fs::write(&script_path, &wrapper) {
        return super::tool_error("execute_app_script", format!("writing temp script: {e}"));
    }

    // Godot requires a project.godot file to run scripts. If a project path
    // is provided, use it. Otherwise create a minimal temporary project.
    let temp_project_dir;
    let project = if let Some(p) = project_path {
        p.to_string()
    } else {
        temp_project_dir = temp_dir.join("llama_chat_godot_project");
        let _ = std::fs::create_dir_all(&temp_project_dir);
        let project_file = temp_project_dir.join("project.godot");
        if !project_file.exists() {
            let _ = std::fs::write(
                &project_file,
                "; Engine configuration file.\n\
                 ; Minimal project for script execution.\n\n\
                 [application]\n\n\
                 config/name=\"LlamaChatTemp\"\n",
            );
        }
        temp_project_dir.to_string_lossy().into_owned()
    };

    let mut cmd = Command::new(exe);
    cmd.args(["--headless", "--path", &project, "--script"]);
    cmd.arg(&script_path);

    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    #[cfg(target_os = "windows")]
    {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let output = match run_command_with_timeout(cmd, SCRIPT_TIMEOUT) {
        Ok(o) => o,
        Err(e) => {
            let _ = std::fs::remove_file(&script_path);
            return super::tool_error("execute_app_script", format!("running Godot: {e}"));
        }
    };

    let _ = std::fs::remove_file(&script_path);

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let filtered_stdout = filter_godot_output(&stdout);
    let filtered_stderr = filter_godot_output(&stderr);

    let status_msg = if output.status.success() {
        "Script completed successfully."
    } else {
        "Script failed."
    };

    let mut result = status_msg.to_string();
    if !filtered_stdout.is_empty() {
        result.push_str(&format!("\n\nOutput:\n{filtered_stdout}"));
    }
    if !filtered_stderr.is_empty() {
        result.push_str(&format!("\n\nErrors:\n{filtered_stderr}"));
    }

    NativeToolResult::text_only(result)
}

/// Find the Godot executable.
fn find_godot_exe() -> Option<String> {
    // 1. Check common command names on PATH
    for name in &["godot", "godot4", "godot3"] {
        let mut check_cmd = Command::new(name);
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
        if check_cmd.status().map(|s| s.success()).unwrap_or(false) {
            return Some(name.to_string());
        }
    }

    // 2. Search common installation paths
    let candidates = find_godot_installations();
    if let Some(latest) = candidates.last() {
        return Some(latest.clone());
    }

    // 3. Generic app discovery fallback
    super::window_tools::find_application_exe("godot")
}

/// Search for Godot installations, returning paths sorted.
fn find_godot_installations() -> Vec<String> {
    let mut results = Vec::new();

    #[cfg(target_os = "windows")]
    {
        // Check common Windows locations
        let search_dirs: Vec<String> = vec![
            std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".to_string()),
            std::env::var("ProgramFiles(x86)")
                .unwrap_or_else(|_| r"C:\Program Files (x86)".to_string()),
            std::env::var("LOCALAPPDATA").unwrap_or_default(),
            std::env::var("USERPROFILE")
                .map(|h| format!("{h}\\Downloads"))
                .unwrap_or_default(),
            std::env::var("SCOOP")
                .map(|s| format!("{s}\\apps\\godot\\current"))
                .unwrap_or_default(),
        ];

        for dir in &search_dirs {
            if dir.is_empty() {
                continue;
            }
            let path = std::path::PathBuf::from(dir);
            if !path.exists() {
                continue;
            }
            if let Ok(entries) = std::fs::read_dir(&path) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_lowercase();
                    if name.contains("godot") {
                        let p = entry.path();
                        if p.is_file() && name.ends_with(".exe") {
                            results.push(p.to_string_lossy().into_owned());
                        } else if p.is_dir() {
                            // Look for exe inside the folder
                            if let Ok(sub_entries) = std::fs::read_dir(&p) {
                                for sub in sub_entries.flatten() {
                                    let sub_name =
                                        sub.file_name().to_string_lossy().to_lowercase();
                                    if sub_name.contains("godot") && sub_name.ends_with(".exe") {
                                        results.push(sub.path().to_string_lossy().into_owned());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Ok(entries) = std::fs::read_dir("/Applications") {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_lowercase();
                if name.contains("godot") && name.ends_with(".app") {
                    let exe = entry
                        .path()
                        .join("Contents")
                        .join("MacOS")
                        .join("Godot");
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
            // Common locations: ~/Godot/, ~/.local/share/godot/
            for subdir in &["Godot", ".local/share/godot", ".local/bin"] {
                let dir = format!("{home}/{subdir}");
                if let Ok(entries) = std::fs::read_dir(&dir) {
                    for entry in entries.flatten() {
                        let name = entry.file_name().to_string_lossy().to_lowercase();
                        if name.contains("godot") {
                            let p = entry.path();
                            if p.is_file() {
                                results.push(p.to_string_lossy().into_owned());
                            }
                        }
                    }
                }
            }
        }
        // Also check /opt
        if let Ok(entries) = std::fs::read_dir("/opt") {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_lowercase();
                if name.contains("godot") {
                    let p = entry.path();
                    if p.is_file() {
                        results.push(p.to_string_lossy().into_owned());
                    } else if p.is_dir() {
                        if let Ok(sub_entries) = std::fs::read_dir(&p) {
                            for sub in sub_entries.flatten() {
                                let sub_name = sub.file_name().to_string_lossy().to_lowercase();
                                if sub_name.contains("godot") && sub.path().is_file() {
                                    results.push(sub.path().to_string_lossy().into_owned());
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    results.sort();
    results
}

/// Filter Godot output noise.
fn filter_godot_output(raw: &str) -> String {
    raw.lines()
        .filter(|line| {
            let l = line.trim();
            !l.is_empty()
                && !l.starts_with("Godot Engine v")
                && !l.starts_with("OpenGL ")
                && !l.starts_with("Vulkan ")
                && !l.starts_with("GLES ")
                && !l.starts_with("RenderingDevice:")
                && !l.starts_with("  Adapter:")
                && !l.starts_with("  Driver:")
                && !l.starts_with("  Device ")
                && !l.starts_with("  Vulkan API")
                && !l.starts_with("Core: ")
                && !l.starts_with("Servers:")
                && !l.starts_with("Scene: ")
                && !l.contains("WARNING: ")
                && !l.contains("_WAS_FREED")
                && !l.contains("cleanup_project_settings")
                && !l.starts_with("  at: ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------------------------------------------------------------------
// Unreal Engine 5 scripting support
// ---------------------------------------------------------------------------

/// Execute a Python script via Unreal Engine 5's `UnrealEditor-Cmd` executable.
///
/// UE5 supports Python scripting via `-ExecutePythonScript`. The user's code is
/// written to a temp `.py` file and passed to the editor in headless mode.
/// If a project path is provided (via `file`), it is passed as the first arg.
fn execute_unreal_script(code: &str, project_path: Option<&str>) -> NativeToolResult {
    let ue_exe = find_unreal_exe();
    let exe = match &ue_exe {
        Some(path) => path.as_str(),
        None => {
            return super::tool_error(
                "execute_app_script",
                "Unreal Engine (UnrealEditor-Cmd) not found.\n\
                 Install UE5 via Epic Games Launcher or ensure UnrealEditor-Cmd is on PATH.",
            )
        }
    };

    // Write code to temp .py file
    let temp_dir = std::env::temp_dir();
    let script_path = temp_dir.join("llama_chat_unreal_script.py");
    if let Err(e) = std::fs::write(&script_path, code) {
        return super::tool_error("execute_app_script", format!("writing temp script: {e}"));
    }

    let script_path_str = script_path.to_string_lossy().to_string();

    let mut cmd = Command::new(exe);

    // If a .uproject file is provided, pass it first
    if let Some(project) = project_path {
        cmd.arg(project);
    }

    cmd.arg(format!("-ExecutePythonScript=\"{script_path_str}\""));
    cmd.args(["-nullrhi", "-stdout", "-unattended", "-nosplash"]);

    // CRITICAL: stdin(null) to prevent pipe inheritance hang
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    // Hide the console window on Windows
    #[cfg(target_os = "windows")]
    {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let output = match run_command_with_timeout(cmd, SCRIPT_TIMEOUT) {
        Ok(o) => o,
        Err(e) => {
            let _ = std::fs::remove_file(&script_path);
            return super::tool_error("execute_app_script", format!("running UnrealEditor-Cmd: {e}"));
        }
    };

    // Clean up temp file
    let _ = std::fs::remove_file(&script_path);

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let filtered_stdout = filter_unreal_output(&stdout);
    let filtered_stderr = filter_unreal_output(&stderr);

    let status_msg = if output.status.success() {
        "Script completed successfully."
    } else {
        "Script failed."
    };

    let mut result = status_msg.to_string();
    if !filtered_stdout.is_empty() {
        result.push_str(&format!("\n\nOutput:\n{filtered_stdout}"));
    }
    if !filtered_stderr.is_empty() {
        result.push_str(&format!("\n\nErrors:\n{filtered_stderr}"));
    }

    NativeToolResult::text_only(result)
}

/// Find the Unreal Engine `UnrealEditor-Cmd` executable.
fn find_unreal_exe() -> Option<String> {
    // 1. Check if "UnrealEditor-Cmd" is on PATH
    let cmd_name = if cfg!(target_os = "windows") {
        "UnrealEditor-Cmd.exe"
    } else {
        "UnrealEditor-Cmd"
    };

    let mut check_cmd = Command::new(cmd_name);
    check_cmd
        .arg("-help")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(target_os = "windows")]
    {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        check_cmd.creation_flags(CREATE_NO_WINDOW);
    }
    if check_cmd.status().map(|s| s.success()).unwrap_or(false) {
        return Some(cmd_name.to_string());
    }

    // 2. Search common installation paths (pick the latest version)
    let candidates = find_unreal_installations();
    if let Some(latest) = candidates.last() {
        return Some(latest.clone());
    }

    // 3. Generic app discovery fallback
    super::window_tools::find_application_exe("UnrealEditor-Cmd")
}

/// Search for Unreal Engine installations, returning UnrealEditor-Cmd paths sorted by version.
fn find_unreal_installations() -> Vec<String> {
    let mut results = Vec::new();

    #[cfg(target_os = "windows")]
    {
        // Epic Games Launcher installs UE5 under C:\Program Files\Epic Games\UE_5.*
        let pf =
            std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".to_string());
        let epic_root = std::path::PathBuf::from(&pf).join("Epic Games");
        if let Ok(entries) = std::fs::read_dir(&epic_root) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with("UE_5") || name.starts_with("UE_4") {
                    let exe = entry
                        .path()
                        .join("Engine")
                        .join("Binaries")
                        .join("Win64")
                        .join("UnrealEditor-Cmd.exe");
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
                    let exe = entry
                        .path()
                        .join("Engine")
                        .join("Binaries")
                        .join("Mac")
                        .join("UnrealEditor-Cmd");
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
            // Common Linux location: ~/UnrealEngine/
            let root = format!("{home}/UnrealEngine");
            let exe_path = std::path::PathBuf::from(&root)
                .join("Engine")
                .join("Binaries")
                .join("Linux")
                .join("UnrealEditor-Cmd");
            if exe_path.is_file() {
                results.push(exe_path.to_string_lossy().into_owned());
            }
        }
    }

    results.sort();
    results
}

/// Filter Unreal Engine output noise (startup messages, shader compilation, etc.).
fn filter_unreal_output(raw: &str) -> String {
    raw.lines()
        .filter(|line| {
            let l = line.trim();
            if l.is_empty() {
                return false;
            }

            // Strip lines from noisy UE5 log categories
            let noisy_prefixes = [
                "LogInit:",
                "LogConfig:",
                "LogPlatformFile:",
                "LogLinker:",
                "LogPackageName:",
                "LogAssetRegistry:",
                "LogShaderCompilers:",
                "LogMaterial:",
                "LogTexture:",
                "LogStreaming:",
                "LogAudio:",
            ];
            for prefix in &noisy_prefixes {
                if l.contains(prefix) {
                    return false;
                }
            }

            // Strip generic noise patterns
            if l.starts_with("Warning:") || l.starts_with("Display:") {
                // Keep Display: lines that look like user print output
                // (e.g. "LogPython: Display: Hello World" would already be caught above)
                // Bare "Display:" lines from engine subsystems are noise
                return false;
            }
            if l.starts_with("Presizing for ") || l.starts_with("Loading ") {
                return false;
            }

            true
        })
        .collect::<Vec<_>>()
        .join("\n")
}
