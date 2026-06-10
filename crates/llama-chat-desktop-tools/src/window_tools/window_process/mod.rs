//! Process and clipboard tools.
//!
//! Provides: `read_clipboard`, `write_clipboard`, `list_processes`, `kill_process`,
//! `open_application`, `get_process_info`, `wait_for_process_exit`,
//! `get_process_tree`, `get_system_metrics`.

mod process_tree;
mod system_metrics;

pub use process_tree::tool_get_process_tree;
pub use system_metrics::tool_get_system_metrics;

use serde_json::Value;

use super::NativeToolResult;
use super::{parse_bool, parse_int};

use super::win32;

// ─── read_clipboard ───────────────────────────────────────────────────────────

/// Read text from the system clipboard, reporting format info.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_read_clipboard(_args: &Value) -> NativeToolResult {
    let formats = win32::get_clipboard_formats();
    let format_str = if formats.is_empty() { "empty".to_string() } else { formats.join("+") };

    if let Ok(files) = win32::read_clipboard_files() {
        if !files.is_empty() {
            let mut output = format!("Format: {format_str}. Clipboard contains {} file(s):\n", files.len());
            for f in &files {
                output.push_str(&format!("  {f}\n"));
            }
            return NativeToolResult::text_only(output);
        }
    }
    match win32::read_clipboard() {
        Ok(text) => {
            let summary = if text.len() > 200 {
                format!("Format: {format_str}. Clipboard ({} chars): \"{}...\"", text.len(), &text[..200])
            } else {
                format!("Format: {format_str}. Clipboard: \"{text}\"")
            };
            NativeToolResult::text_only(summary)
        }
        Err(e) => super::tool_error("read_clipboard", format!("Format: {format_str}. {e}")),
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_read_clipboard(_args: &Value) -> NativeToolResult {
    super::tool_error("read_clipboard", "not available on this platform")
}

// ─── write_clipboard ──────────────────────────────────────────────────────────

/// Write text to the system clipboard.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_write_clipboard(args: &Value) -> NativeToolResult {
    let text = match args.get("text").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return super::tool_error("write_clipboard", "'text' argument is required"),
    };

    match win32::write_clipboard(text) {
        Ok(()) => {
            let summary = if text.len() > 50 {
                format!("Wrote {} chars to clipboard", text.len())
            } else {
                format!("Wrote to clipboard: \"{text}\"")
            };
            NativeToolResult::text_only(summary)
        }
        Err(e) => super::tool_error("write_clipboard", e),
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_write_clipboard(_args: &Value) -> NativeToolResult {
    super::tool_error("write_clipboard", "not available on this platform")
}

// ─── list_processes ───────────────────────────────────────────────────────────

/// List running processes, optionally filtered by name.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_list_processes(args: &Value) -> NativeToolResult {
    let filter = args.get("filter").and_then(|v| v.as_str()).map(|s| s.to_lowercase());

    match win32::enumerate_processes() {
        Ok(procs) => {
            let mut filtered: Vec<_> = procs.into_iter()
                .filter(|(_, name)| {
                    filter.as_ref().is_none_or(|f| name.to_lowercase().contains(f))
                })
                .collect();
            filtered.sort_by_key(|a| a.1.to_lowercase());

            let total = filtered.len();
            let limited = if total > 100 { &filtered[..100] } else { &filtered };

            let lines: Vec<String> = limited.iter().map(|(pid, name)| {
                format!("  PID {pid:>6}  {name}")
            }).collect();

            let suffix = if total > 100 {
                format!("\n... and {} more (use filter to narrow)", total - 100)
            } else {
                String::new()
            };
            let count = limited.len();
            let joined = lines.join("\n");
            NativeToolResult::text_only(format!("{count} process(es):\n{joined}{suffix}"))
        }
        Err(e) => super::tool_error("list_processes", e),
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_list_processes(_args: &Value) -> NativeToolResult {
    super::tool_error("list_processes", "not available on this platform")
}

// ─── kill_process ─────────────────────────────────────────────────────────────

/// Terminate a process by name or PID. Refuses to kill system-critical processes.
/// Supports graceful shutdown via `force=false` (sends WM_CLOSE/SIGTERM, then waits).
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_kill_process(args: &Value) -> NativeToolResult {
    let name_filter = args.get("name").and_then(|v| v.as_str());
    let pid = args.get("pid").and_then(parse_int).map(|v| v as u32);
    let force = args.get("force").map(|v| parse_bool(v, true)).unwrap_or(true);
    let grace_ms = args.get("grace_ms").and_then(parse_int)
        .unwrap_or(5000)
        .clamp(500, 15000) as u64;

    if name_filter.is_none() && pid.is_none() {
        return super::tool_error("kill_process", "'name' or 'pid' is required");
    }

    const PROTECTED: &[&str] = &[
        "csrss.exe", "lsass.exe", "smss.exe", "svchost.exe", "dwm.exe",
        "winlogon.exe", "wininit.exe", "services.exe", "system",
        "explorer.exe", "conhost.exe",
    ];

    let current_pid = std::process::id();

    if let Some(target_pid) = pid {
        if target_pid == current_pid {
            return super::tool_error("kill_process", "refusing to kill own process");
        }
        if let Ok(procs) = win32::enumerate_processes() {
            if let Some((_, name)) = procs.iter().find(|(p, _)| *p == target_pid) {
                if PROTECTED.iter().any(|&p| name.to_lowercase() == p) {
                    return super::tool_error("kill_process", format!("refusing to kill system-critical process '{name}' (PID {target_pid})"));
                }
            }
        }
        if force {
            match win32::terminate_process(target_pid) {
                Ok(()) => NativeToolResult::text_only(format!("Terminated process PID {target_pid}")),
                Err(e) => super::tool_error("kill_process", e),
            }
        } else {
            graceful_kill_pid(target_pid, grace_ms)
        }
    } else if let Some(name) = name_filter {
        let name_lower = name.to_lowercase();
        if PROTECTED.iter().any(|&p| name_lower == p || name_lower == p.trim_end_matches(".exe")) {
            return super::tool_error("kill_process", format!("refusing to kill system-critical process '{name}'"));
        }
        match win32::enumerate_processes() {
            Ok(procs) => {
                let targets: Vec<_> = procs.into_iter()
                    .filter(|(p, n)| *p != current_pid && n.to_lowercase().contains(&name_lower))
                    .collect();
                if targets.is_empty() {
                    return NativeToolResult::text_only(format!("No process matching '{name}' found"));
                }
                if force {
                    let mut killed = 0;
                    let mut errors = Vec::new();
                    for (p, n) in &targets {
                        match win32::terminate_process(*p) {
                            Ok(()) => killed += 1,
                            Err(e) => errors.push(format!("PID {p} ({n}): {e}")),
                        }
                    }
                    let mut msg = format!("Killed {killed}/{} process(es) matching '{name}'", targets.len());
                    if !errors.is_empty() {
                        let errs = errors.join("; ");
                        msg.push_str(&format!("\nErrors: {errs}"));
                    }
                    NativeToolResult::text_only(msg)
                } else {
                    let mut results = Vec::new();
                    for (p, n) in &targets {
                        let r = graceful_kill_pid(*p, grace_ms);
                        let text = &r.text;
                        results.push(format!("PID {p} ({n}): {text}"));
                    }
                    NativeToolResult::text_only(format!(
                        "Graceful kill for {} process(es) matching '{name}':\n{}",
                        targets.len(), results.join("\n")
                    ))
                }
            }
            Err(e) => super::tool_error("kill_process", e),
        }
    } else {
        super::tool_error("kill_process", "unreachable")
    }
}

/// Gracefully terminate a process: send close signals, wait, then force kill if needed.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
fn graceful_kill_pid(target_pid: u32, grace_ms: u64) -> NativeToolResult {
    #[cfg(windows)]
    {
        let hwnds = win32::find_hwnds_by_pid(target_pid);
        let window_count = hwnds.len();
        for hwnd in hwnds {
            win32::close_window_graceful(hwnd);
        }
        if window_count == 0 {
            return match win32::terminate_process(target_pid) {
                Ok(()) => NativeToolResult::text_only(format!(
                    "No windows for PID {target_pid}; force-terminated"
                )),
                Err(e) => super::tool_error("kill_process", e),
            };
        }
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("kill")
            .args(["-TERM", &target_pid.to_string()])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }

    let start = std::time::Instant::now();
    loop {
        std::thread::sleep(std::time::Duration::from_millis(200));
        if !win32::is_process_alive(target_pid) {
            let elapsed = start.elapsed().as_millis();
            return NativeToolResult::text_only(format!(
                "Process PID {target_pid} exited gracefully after {elapsed}ms"
            ));
        }
        if start.elapsed().as_millis() as u64 >= grace_ms {
            break;
        }
    }

    match win32::terminate_process(target_pid) {
        Ok(()) => NativeToolResult::text_only(format!(
            "Process PID {target_pid} did not exit within {grace_ms}ms; force-terminated"
        )),
        Err(e) => NativeToolResult::text_only(format!(
            "Process PID {target_pid} did not exit within {grace_ms}ms; force-kill failed: {e}"
        )),
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_kill_process(_args: &Value) -> NativeToolResult {
    super::tool_error("kill_process", "not available on this platform")
}

// ─── open_application ────────────────────────────────────────────────────────

/// Open/launch an application by name or path. With `capture_output: true`, captures stdout/stderr.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_open_application(args: &Value) -> NativeToolResult {
    let target = match args.get("target").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return super::tool_error("open_application", "'target' argument is required (app name or path)"),
    };
    let arguments = args.get("args").and_then(|v| v.as_str());
    let capture_output = super::parse_bool(
        args.get("capture_output").unwrap_or(&serde_json::json!(false)),
        false,
    );

    if let Some(gpu) = super::gpu_app_db::detect_gpu_app_by_target(target) {
        if super::gpu_app_db::is_gpu_app_running(gpu) {
            let guidance = super::gpu_app_db::build_guidance(gpu);
            return NativeToolResult::text_only(format!(
                "{} is already running. A new instance was not opened.\n\
                 You can interact with the existing instance.\n\n{}",
                gpu.app_name, guidance
            ));
        }
    }

    if capture_output {
        let mut cmd = std::process::Command::new(target);
        cmd.stdin(std::process::Stdio::null());
        if let Some(a) = arguments {
            for part in a.split_whitespace() {
                cmd.arg(part);
            }
        }
        match cmd.output() {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                let mut result = format!("Exit code: {}\n", output.status.code().unwrap_or(-1));
                if !stdout.is_empty() {
                    let trunc = if stdout.len() > 4000 { &stdout[..4000] } else { &stdout };
                    result.push_str(&format!("stdout:\n{trunc}\n"));
                }
                if !stderr.is_empty() {
                    let trunc = if stderr.len() > 2000 { &stderr[..2000] } else { &stderr };
                    result.push_str(&format!("stderr:\n{trunc}\n"));
                }
                NativeToolResult::text_only(result)
            }
            Err(e) => super::tool_error("open_application", format!("running '{target}': {e}")),
        }
    } else {
        match win32::shell_execute(target, arguments) {
            Ok(()) => {
                let desc = if let Some(a) = arguments {
                    format!("Launched '{target}' with args '{a}'")
                } else {
                    format!("Launched '{target}'")
                };
                NativeToolResult::text_only(desc)
            }
            Err(_) => {
                match find_application_exe(target) {
                    Some(found_path) => {
                        match win32::shell_execute(&found_path, arguments) {
                            Ok(()) => {
                                let desc = if let Some(a) = arguments {
                                    format!("Launched '{target}' from '{found_path}' with args '{a}'")
                                } else {
                                    format!("Launched '{target}' from '{found_path}'")
                                };
                                NativeToolResult::text_only(desc)
                            }
                            Err(e2) => super::tool_error("open_application", format!("found '{found_path}' but failed to launch: {e2}")),
                        }
                    }
                    None => super::tool_error("open_application", format!(
                        "'{target}' not found. Not in PATH, registry, or Program Files. \
                         Try providing the full path to the executable."
                    )),
                }
            }
        }
    }
}

/// Search for an application executable by name in common installation directories.
/// Returns the full path if found, None otherwise.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn find_application_exe(name: &str) -> Option<String> {
    let name_lower = name.to_lowercase();
    let base_name = name_lower.strip_suffix(".exe").unwrap_or(&name_lower);

    let search_dirs: Vec<std::path::PathBuf> = {
        let mut dirs = Vec::new();
        #[cfg(windows)]
        {
            if let Ok(pf) = std::env::var("ProgramFiles") {
                dirs.push(std::path::PathBuf::from(pf));
            }
            if let Ok(pf86) = std::env::var("ProgramFiles(x86)") {
                dirs.push(std::path::PathBuf::from(pf86));
            }
            if let Ok(local) = std::env::var("LOCALAPPDATA") {
                dirs.push(std::path::PathBuf::from(local));
            }
        }
        #[cfg(target_os = "macos")]
        {
            dirs.push(std::path::PathBuf::from("/Applications"));
            dirs.push(std::path::PathBuf::from("/usr/local/bin"));
        }
        #[cfg(target_os = "linux")]
        {
            dirs.push(std::path::PathBuf::from("/usr/bin"));
            dirs.push(std::path::PathBuf::from("/usr/local/bin"));
            dirs.push(std::path::PathBuf::from("/opt"));
            dirs.push(std::path::PathBuf::from("/snap/bin"));
        }
        dirs
    };

    #[cfg(windows)]
    let exe_name = format!("{base_name}.exe");
    #[cfg(not(windows))]
    let exe_name = base_name.to_string();

    for dir in &search_dirs {
        if !dir.exists() {
            continue;
        }
        let direct = dir.join(&exe_name);
        if direct.is_file() {
            return Some(direct.to_string_lossy().into_owned());
        }
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let entry_name = entry.file_name().to_string_lossy().to_lowercase();
                if entry_name.contains(base_name) && entry.path().is_dir() {
                    if let Some(found) = find_exe_in_dir(&entry.path(), &exe_name, 2) {
                        return Some(found);
                    }
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        let app_path = format!("/Applications/{}.app/Contents/MacOS/{}",
            capitalize_first(base_name), capitalize_first(base_name));
        if std::path::Path::new(&app_path).is_file() {
            return Some(app_path);
        }
        let app_path_lower = format!("/Applications/{}.app/Contents/MacOS/{}",
            capitalize_first(base_name), base_name);
        if std::path::Path::new(&app_path_lower).is_file() {
            return Some(app_path_lower);
        }
    }

    None
}

/// Recursively search for an executable file in a directory, up to max_depth levels.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
fn find_exe_in_dir(dir: &std::path::Path, exe_name: &str, max_depth: u32) -> Option<String> {
    let candidate = dir.join(exe_name);
    if candidate.is_file() {
        return Some(candidate.to_string_lossy().into_owned());
    }
    if max_depth == 0 {
        return None;
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                if let Some(found) = find_exe_in_dir(&entry.path(), exe_name, max_depth - 1) {
                    return Some(found);
                }
            }
        }
    }
    None
}

/// Capitalize the first letter of a string (for macOS .app bundle names).
#[cfg(target_os = "macos")]
fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().to_string() + chars.as_str(),
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_open_application(_args: &Value) -> NativeToolResult {
    super::tool_error("open_application", "not available on this platform")
}

// ─── get_process_info ────────────────────────────────────────────────────────

/// Get resource info (memory, CPU time) for a process by PID or name.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_get_process_info(args: &Value) -> NativeToolResult {
    let pid = args.get("pid").and_then(parse_int).map(|v| v as u32);
    let name = args.get("name").and_then(|v| v.as_str());

    let target_pid = if let Some(p) = pid {
        p
    } else if let Some(n) = name {
        let lower = n.to_lowercase();
        match win32::enumerate_processes() {
            Ok(procs) => {
                match procs.iter().find(|(_, pname)| pname.to_lowercase().contains(&lower)) {
                    Some((p, _)) => *p,
                    None => return super::tool_error("get_process_info", format!("no process matching '{n}'")),
                }
            }
            Err(e) => return super::tool_error("get_process_info", e),
        }
    } else {
        return super::tool_error("get_process_info", "'pid' or 'name' is required");
    };

    match win32::get_process_resource_info(target_pid) {
        Ok((working_set, kernel_ms, user_ms)) => {
            let mb = working_set as f64 / (1024.0 * 1024.0);
            let total_cpu = kernel_ms + user_ms;
            NativeToolResult::text_only(format!(
                "PID {target_pid}: memory={mb:.1}MB, kernel_time={kernel_ms}ms, user_time={user_ms}ms, total_cpu={total_cpu}ms"
            ))
        }
        Err(e) => super::tool_error("get_process_info", e),
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_get_process_info(_args: &Value) -> NativeToolResult {
    super::tool_error("get_process_info", "not available on this platform")
}

// ─── wait_for_process_exit ───────────────────────────────────────────────────

/// Wait until a process exits or timeout.
/// Params: `pid` (integer) or `name` (string), `timeout_ms` (integer, default 30000, max 120000).
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_wait_for_process_exit(args: &Value) -> NativeToolResult {
    let pid = args.get("pid").and_then(parse_int).map(|v| v as u32);
    let name = args.get("name").and_then(|v| v.as_str());

    let timeout_ms = args
        .get("timeout_ms")
        .and_then(parse_int)
        .unwrap_or(30000)
        .clamp(500, 120000) as u64;

    let target_pid = if let Some(p) = pid {
        p
    } else if let Some(n) = name {
        let lower = n.to_lowercase();
        match win32::enumerate_processes() {
            Ok(procs) => {
                match procs.iter().find(|(_, pname)| pname.to_lowercase().contains(&lower)) {
                    Some((p, _)) => *p,
                    None => return NativeToolResult::text_only(format!(
                        "No running process matching '{n}' — may have already exited"
                    )),
                }
            }
            Err(e) => return super::tool_error("wait_for_process_exit", e),
        }
    } else {
        return super::tool_error("wait_for_process_exit", "'pid' or 'name' is required");
    };

    if !win32::is_process_alive(target_pid) {
        return NativeToolResult::text_only(format!(
            "Process {target_pid} already exited (not running)"
        ));
    }

    let start = std::time::Instant::now();
    let poll_interval = std::time::Duration::from_millis(500);

    loop {
        if let Err(e) = super::ensure_desktop_not_cancelled() {
            return super::tool_error("wait_for_process_exit", e);
        }
        if let Err(e) = super::interruptible_sleep(poll_interval) {
            return super::tool_error("wait_for_process_exit", e);
        }

        let elapsed = start.elapsed().as_millis() as u64;

        if !win32::is_process_alive(target_pid) {
            return NativeToolResult::text_only(format!(
                "Process {target_pid} exited after {elapsed}ms"
            ));
        }
        if elapsed >= timeout_ms {
            return NativeToolResult::text_only(format!(
                "Process {target_pid} still running after {timeout_ms}ms (timeout)"
            ));
        }
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_wait_for_process_exit(_args: &Value) -> NativeToolResult {
    super::tool_error("wait_for_process_exit", "not available on this platform")
}
