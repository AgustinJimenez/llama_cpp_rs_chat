//! Database of GPU-rendered applications that bypass Windows UI Automation.
//!
//! Apps like Blender, Unity, Unreal, Godot, and Maya render their UI with OpenGL/Vulkan/DirectX.
//! Windows UI Automation returns nothing useful for them. This module detects such apps early
//! so we can return helpful guidance instead of wasting tool calls on empty results.

/// Info about a known GPU-rendered application.
#[allow(dead_code)]
pub struct GpuAppInfo {
    pub app_name: &'static str,
    /// Scripting language if available (e.g. "Python (bpy)")
    pub script_lang: Option<&'static str>,
    /// CLI flag to run a script (e.g. "--python")
    pub script_cli_flag: Option<&'static str>,
    /// CLI flag for headless/background mode (e.g. "--background")
    pub background_flag: Option<&'static str>,
}

/// Generate guidance message for an LLM when a GPU-rendered app is detected.
/// Informative, not imperative — the model decides what to do.
pub fn build_guidance(gpu: &GpuAppInfo) -> String {
    let mut msg = format!(
        "Note: {} renders its UI with GPU, so UI Automation tools (get_ui_tree, find_ui_elements, \
         fill_form, etc.) will not return useful results.\n\n\
         Tools that work: take_screenshot, click_screen, press_key, type_text, scroll_screen, ocr_screen",
        gpu.app_name
    );
    if let Some(lang) = gpu.script_lang {
        msg.push_str(&format!(
            ", execute_app_script\n\n{} supports {} scripting via execute_app_script.",
            gpu.app_name, lang
        ));
        if let Some(cli) = gpu.script_cli_flag {
            msg.push_str(&format!(" CLI flag: {}", cli));
        }
    }
    msg
}

/// Static database of known GPU-rendered applications.
/// Matched by window class name or process name (case-insensitive).
static GPU_APPS: &[(&str, &str, GpuAppInfo)] = &[
    // (class_name_pattern, process_name_pattern, info)
    // Blender
    ("ghost_windowclass", "blender", GpuAppInfo {
        app_name: "Blender",
        script_lang: Some("Python (bpy)"),
        script_cli_flag: Some("--python"),
        background_flag: Some("--background"),
    }),
    ("unitywndclass", "unity", GpuAppInfo {
        app_name: "Unity",
        script_lang: Some("C#"),
        script_cli_flag: Some("-executeMethod"),
        background_flag: Some("-batchmode"),
    }),
    ("unrealwindow", "unrealed", GpuAppInfo {
        app_name: "Unreal Engine",
        script_lang: Some("Python"),
        script_cli_flag: Some("-ExecutePythonScript"),
        background_flag: None,
    }),
    ("godot", "godot", GpuAppInfo {
        app_name: "Godot",
        script_lang: Some("GDScript"),
        script_cli_flag: Some("--script"),
        background_flag: Some("--headless"),
    }),
    ("", "maya", GpuAppInfo {
        app_name: "Maya",
        script_lang: Some("Python/MEL"),
        script_cli_flag: Some("-command"),
        background_flag: Some("-batch"),
    }),
    ("", "houdini", GpuAppInfo {
        app_name: "Houdini",
        script_lang: Some("Python (hou)"),
        script_cli_flag: None,
        background_flag: None,
    }),
    ("", "3dsmax", GpuAppInfo {
        app_name: "3ds Max",
        script_lang: Some("MAXScript/Python"),
        script_cli_flag: Some("-u MAXScript"),
        background_flag: None,
    }),
    ("", "cinema 4d", GpuAppInfo {
        app_name: "Cinema 4D",
        script_lang: Some("Python"),
        script_cli_flag: None,
        background_flag: None,
    }),
    ("", "resolve", GpuAppInfo {
        app_name: "DaVinci Resolve",
        script_lang: Some("Python (DaVinci Resolve API)"),
        script_cli_flag: None,
        background_flag: None,
    }),
    ("", "substance", GpuAppInfo {
        app_name: "Substance",
        script_lang: Some("Python"),
        script_cli_flag: None,
        background_flag: None,
    }),
];

/// Detect if a target name (from open_application) matches a known GPU app.
/// Matches against process_name patterns in the database.
pub fn detect_gpu_app_by_target(target: &str) -> Option<&'static GpuAppInfo> {
    let target_lower = target.to_lowercase();
    let base = target_lower
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(&target_lower)
        .strip_suffix(".exe")
        .unwrap_or(&target_lower);

    for (_class_pattern, proc_pattern, info) in GPU_APPS {
        if !proc_pattern.is_empty() && base.contains(proc_pattern) {
            return Some(info);
        }
    }
    None
}

/// Detect if a window belongs to a GPU-rendered application.
/// Returns app info if matched by class name or process name.
pub fn detect_gpu_app(class_name: &str, process_name: &str) -> Option<&'static GpuAppInfo> {
    let class_lower = class_name.to_lowercase();
    let proc_lower = process_name.to_lowercase();

    for (class_pattern, proc_pattern, info) in GPU_APPS {
        if !class_pattern.is_empty() && class_lower.contains(class_pattern) {
            return Some(info);
        }
        if !proc_pattern.is_empty() && proc_lower.contains(proc_pattern) {
            return Some(info);
        }
    }
    None
}

/// Check if a GPU app is already running by scanning visible windows.
/// Returns true if any window's process_name matches the GPU app's pattern.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn is_gpu_app_running(gpu: &GpuAppInfo) -> bool {
    #[cfg(windows)]
    use super::win32;
    #[cfg(target_os = "macos")]
    use super::macos as win32;
    #[cfg(target_os = "linux")]
    use super::linux as win32;

    let windows = win32::enumerate_windows();
    for w in &windows {
        if detect_gpu_app(&w.class_name, &w.process_name)
            .map_or(false, |g| g.app_name == gpu.app_name)
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_blender_by_class() {
        let info = detect_gpu_app("GHOST_WindowClass", "blender.exe");
        assert!(info.is_some());
        assert_eq!(info.unwrap().app_name, "Blender");
    }

    #[test]
    fn test_detect_blender_by_process() {
        let info = detect_gpu_app("SomeClass", "blender");
        assert!(info.is_some());
        assert_eq!(info.unwrap().app_name, "Blender");
    }

    #[test]
    fn test_detect_unity_by_class() {
        let info = detect_gpu_app("UnityWndClass", "unity.exe");
        assert!(info.is_some());
        assert_eq!(info.unwrap().app_name, "Unity");
    }

    #[test]
    fn test_no_match_for_notepad() {
        let info = detect_gpu_app("Notepad", "notepad.exe");
        assert!(info.is_none());
    }
}
