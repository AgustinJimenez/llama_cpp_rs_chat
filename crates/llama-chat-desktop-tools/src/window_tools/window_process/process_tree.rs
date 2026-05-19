//! Process tree tool: show a process and all its children recursively.

use serde_json::Value;

use crate::NativeToolResult;
use crate::parse_int;
use crate::tool_error;

#[cfg(windows)]
use crate::win32;
#[cfg(target_os = "macos")]
use crate::macos as win32;
#[cfg(target_os = "linux")]
use crate::linux as win32;

/// Show a process and all its children recursively as a tree.
/// Params: `pid` (integer, required).
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_get_process_tree(args: &Value) -> NativeToolResult {
    let pid = match args.get("pid").and_then(parse_int) {
        Some(p) => p as u32,
        None => return tool_error("get_process_tree", "'pid' is required"),
    };

    let root_name = match win32::enumerate_processes() {
        Ok(procs) => procs
            .iter()
            .find(|(p, _)| *p == pid)
            .map(|(_, n)| n.clone())
            .unwrap_or_else(|| "unknown".to_string()),
        Err(e) => return tool_error("get_process_tree", e),
    };

    if root_name == "unknown" {
        return tool_error("get_process_tree", format!("Process {} not found", pid));
    }

    #[cfg(windows)]
    let tree_output = build_process_tree_windows(pid, &root_name);

    #[cfg(not(windows))]
    let tree_output = build_process_tree_unix(pid, &root_name);

    NativeToolResult::text_only(tree_output)
}

/// Build process tree on Windows using PowerShell to query parent-child relationships.
#[cfg(windows)]
fn build_process_tree_windows(root_pid: u32, root_name: &str) -> String {
    use std::process::{Command, Stdio};
    use std::os::windows::process::CommandExt;

    let ps_script = r#"Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,Name | ConvertTo-Csv -NoTypeInformation"#;

    let mut cmd = Command::new("powershell");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", ps_script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(0x08000000); // CREATE_NO_WINDOW

    let output = match cmd.output() {
        Ok(o) => o,
        Err(_) => return format!("PID {}: {}\n  (child enumeration unavailable)", root_pid, root_name),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut parent_map: std::collections::HashMap<u32, Vec<(u32, String)>> = std::collections::HashMap::new();

    for line in stdout.lines().skip(1) {
        let fields: Vec<&str> = line.split(',').collect();
        if fields.len() >= 3 {
            let child_pid: u32 = fields[0].trim_matches('"').parse().unwrap_or(0);
            let parent_pid: u32 = fields[1].trim_matches('"').parse().unwrap_or(0);
            let name = fields[2].trim_matches('"').to_string();
            if child_pid != 0 {
                parent_map.entry(parent_pid).or_default().push((child_pid, name));
            }
        }
    }

    let mut output = String::new();
    format_tree_recursive(&parent_map, root_pid, root_name, "", true, &mut output, 0);
    if output.is_empty() {
        format!("PID {}: {} (no children)", root_pid, root_name)
    } else {
        output
    }
}

/// Build process tree on macOS/Linux using ps.
#[cfg(not(windows))]
fn build_process_tree_unix(root_pid: u32, root_name: &str) -> String {
    use std::process::{Command, Stdio};

    let mut parent_map: std::collections::HashMap<u32, Vec<(u32, String)>> = std::collections::HashMap::new();

    let output = Command::new("ps")
        .args(["-eo", "pid,ppid,comm"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output();

    if let Ok(out) = output {
        let stdout = String::from_utf8_lossy(&out.stdout);
        for line in stdout.lines().skip(1) {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 {
                let child_pid: u32 = parts[0].parse().unwrap_or(0);
                let parent_pid: u32 = parts[1].parse().unwrap_or(0);
                let name = parts[2..].join(" ");
                if child_pid != 0 {
                    parent_map.entry(parent_pid).or_default().push((child_pid, name));
                }
            }
        }
    }

    let mut output = String::new();
    format_tree_recursive(&parent_map, root_pid, root_name, "", true, &mut output, 0);
    if output.is_empty() {
        format!("PID {}: {} (no children)", root_pid, root_name)
    } else {
        output
    }
}

/// Recursively format a process tree with indentation.
fn format_tree_recursive(
    parent_map: &std::collections::HashMap<u32, Vec<(u32, String)>>,
    pid: u32,
    name: &str,
    prefix: &str,
    is_root: bool,
    output: &mut String,
    depth: usize,
) {
    if depth > 20 {
        return;
    }

    if is_root {
        output.push_str(&format!("PID {}: {}\n", pid, name));
    } else {
        output.push_str(&format!("{}PID {}: {}\n", prefix, pid, name));
    }

    if let Some(children) = parent_map.get(&pid) {
        let count = children.len();
        for (i, (child_pid, child_name)) in children.iter().enumerate() {
            let is_last = i == count - 1;
            let connector = if is_last { "  " } else { "  " };
            let child_prefix = if is_root {
                format!("  {}", connector)
            } else {
                format!("{}  {}", prefix, connector)
            };
            format_tree_recursive(parent_map, *child_pid, child_name, &child_prefix, false, output, depth + 1);
        }
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_get_process_tree(_args: &Value) -> NativeToolResult {
    tool_error("get_process_tree", "not available on this platform")
}
