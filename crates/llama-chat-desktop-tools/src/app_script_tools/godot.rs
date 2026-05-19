//! Godot GDScript execution (headless --script mode).

use std::process::{Command, Stdio};

use super::super::NativeToolResult;
use super::common::{apply_no_window, canonicalize_project_path, run_command_with_timeout, SCRIPT_TIMEOUT};

/// Execute a GDScript via Godot's `--headless --script` mode.
pub fn execute_godot_script(code: &str, project_path: Option<&str>) -> NativeToolResult {
    let godot_exe = find_godot_exe();
    let exe = match &godot_exe {
        Some(path) => path.as_str(),
        None => return super::super::tool_error(
            "execute_app_script", "Godot not found. Install Godot or ensure it's on PATH.",
        ),
    };

    let indented_code = code
        .lines()
        .map(|line| {
            if line.trim().is_empty() { String::new() } else { format!("\t{line}") }
        })
        .collect::<Vec<_>>()
        .join("\n");

    let wrapper = format!(
        "extends SceneTree\n\nfunc _init():\n{indented_code}\n\tquit()\n"
    );

    let temp_dir = std::env::temp_dir();
    let script_path = temp_dir.join("llama_chat_godot_script.gd");
    if let Err(e) = std::fs::write(&script_path, &wrapper) {
        return super::super::tool_error("execute_app_script", format!("writing temp script: {e}"));
    }

    let temp_project_dir;
    let project = if let Some(p) = project_path {
        match canonicalize_project_path(p) {
            Ok(canonical) => canonical,
            Err(e) => return super::super::tool_error("execute_app_script", e),
        }
    } else {
        temp_project_dir = temp_dir.join("llama_chat_godot_project");
        let _ = std::fs::create_dir_all(&temp_project_dir);
        let project_file = temp_project_dir.join("project.godot");
        if !project_file.exists() {
            let _ = std::fs::write(
                &project_file,
                "; Engine configuration file.\n; Minimal project for script execution.\n\n[application]\n\nconfig/name=\"LlamaChatTemp\"\n",
            );
        }
        temp_project_dir.to_string_lossy().into_owned()
    };

    let mut cmd = Command::new(exe);
    cmd.args(["--headless", "--path", &project, "--script"]);
    cmd.arg(&script_path);
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    apply_no_window(&mut cmd);

    let output = match run_command_with_timeout(cmd, SCRIPT_TIMEOUT) {
        Ok(o) => o,
        Err(e) => {
            let _ = std::fs::remove_file(&script_path);
            return super::super::tool_error("execute_app_script", format!("running Godot: {e}"));
        }
    };

    let _ = std::fs::remove_file(&script_path);

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let filtered_stdout = filter_godot_output(&stdout);
    let filtered_stderr = filter_godot_output(&stderr);

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

pub fn find_godot_exe() -> Option<String> {
    for name in &["godot", "godot4", "godot3"] {
        let mut check_cmd = Command::new(name);
        check_cmd.arg("--version").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        apply_no_window(&mut check_cmd);
        if check_cmd.status().map(|s| s.success()).unwrap_or(false) {
            return Some(name.to_string());
        }
    }

    let candidates = find_godot_installations();
    if let Some(latest) = candidates.last() {
        return Some(latest.clone());
    }
    super::super::window_tools::find_application_exe("godot")
}

fn find_godot_installations() -> Vec<String> {
    let mut results = Vec::new();

    #[cfg(target_os = "windows")]
    {
        let search_dirs: Vec<String> = vec![
            std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".to_string()),
            std::env::var("ProgramFiles(x86)").unwrap_or_else(|_| r"C:\Program Files (x86)".to_string()),
            std::env::var("LOCALAPPDATA").unwrap_or_default(),
            std::env::var("USERPROFILE").map(|h| format!("{h}\\Downloads")).unwrap_or_default(),
            std::env::var("SCOOP").map(|s| format!("{s}\\apps\\godot\\current")).unwrap_or_default(),
        ];

        for dir in &search_dirs {
            if dir.is_empty() { continue; }
            let path = std::path::PathBuf::from(dir);
            if !path.exists() { continue; }
            if let Ok(entries) = std::fs::read_dir(&path) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_lowercase();
                    if name.contains("godot") {
                        let p = entry.path();
                        if p.is_file() && name.ends_with(".exe") {
                            results.push(p.to_string_lossy().into_owned());
                        } else if p.is_dir() {
                            if let Ok(sub_entries) = std::fs::read_dir(&p) {
                                for sub in sub_entries.flatten() {
                                    let sub_name = sub.file_name().to_string_lossy().to_lowercase();
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
                    let exe = entry.path().join("Contents").join("MacOS").join("Godot");
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
