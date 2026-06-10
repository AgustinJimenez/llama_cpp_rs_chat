//! Execute scripts inside GPU-rendered applications (Blender bpy, Unity C#, Maya Python, Godot GDScript, Unreal Engine 5 Python).
//!
//! These apps render their UI with OpenGL/Vulkan, so UI Automation doesn't work.
//! Instead, we invoke their built-in scripting engines via CLI.

use serde_json::Value;

use super::NativeToolResult;
use super::gpu_app_db;

mod common;
mod blender;
mod unity;
mod maya;
mod godot;
mod unreal;

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
        "blender" => blender::execute_blender_script(code, file, background),
        "unity" => unity::execute_unity_script(code, file),
        "maya" => maya::execute_maya_script(code, file),
        "godot" => godot::execute_godot_script(code, file),
        "unreal" | "ue" | "ue5" => unreal::execute_unreal_script(code, file),
        other => {
            if let Some(info) = gpu_app_db::detect_gpu_app("", other) {
                let app_name = info.app_name;
                if info.script_lang.is_some() {
                    let cli_flag = info.script_cli_flag.unwrap_or("");
                    super::tool_error("execute_app_script", format!(
                        "{app_name} scripting via execute_app_script is not yet implemented.\n\
                         Use execute_command to run it manually: {other} {cli_flag} <script>",
                    ))
                } else {
                    super::tool_error("execute_app_script", format!(
                        "{app_name} does not have built-in script support via this tool.",
                    ))
                }
            } else {
                super::tool_error("execute_app_script", format!(
                    "unknown app '{other}'. Supported: blender, unity, maya, godot, unreal",
                ))
            }
        }
    }
}
