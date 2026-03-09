//! System-level tools: registry, system tray, window monitoring.

use serde_json::Value;

use super::NativeToolResult;
use super::parse_int;

/// Read a value from the Windows registry.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_read_registry(args: &Value) -> NativeToolResult {
    #[cfg(windows)]
    use super::win32;
    #[cfg(target_os = "macos")]
    use super::macos as win32;
    #[cfg(target_os = "linux")]
    use super::linux as win32;

    let hive_str = args
        .get("hive")
        .and_then(|v| v.as_str())
        .unwrap_or("HKCU");
    let key = match args.get("key").and_then(|v| v.as_str()) {
        Some(k) => k,
        None => return NativeToolResult::text_only("Error: 'key' (subkey path) is required".to_string()),
    };
    let value_name = args.get("value").and_then(|v| v.as_str()).unwrap_or("");

    let hkey_root = match hive_str.to_uppercase().as_str() {
        "HKCU" | "HKEY_CURRENT_USER" => win32::HKEY_CURRENT_USER,
        "HKLM" | "HKEY_LOCAL_MACHINE" => win32::HKEY_LOCAL_MACHINE,
        other => {
            return NativeToolResult::text_only(format!(
                "Error: unsupported hive '{}'. Use HKCU or HKLM",
                other
            ))
        }
    };

    match win32::read_registry_value(hkey_root, key, value_name) {
        Ok(val) => NativeToolResult::text_only(format!(
            "Registry {}\\{}\\{} = \"{}\"",
            hive_str,
            key,
            if value_name.is_empty() { "(Default)" } else { value_name },
            val
        )),
        Err(e) => NativeToolResult::text_only(format!("Error: {e}")),
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_read_registry(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: read_registry is not available on this platform".to_string())
}

/// Click a system tray icon by tooltip/name text.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_click_tray_icon(args: &Value) -> NativeToolResult {
    use super::ui_tools;
    #[cfg(windows)]
    use super::win32;
    #[cfg(target_os = "macos")]
    use super::macos as win32;
    #[cfg(target_os = "linux")]
    use super::linux as win32;

    let name = match args.get("name").and_then(|v| v.as_str()) {
        Some(n) => n,
        None => return NativeToolResult::text_only("Error: 'name' (icon tooltip text) is required".to_string()),
    };

    // Find the system tray: Shell_TrayWnd → TrayNotifyWnd → SysPager → ToolbarWindow32
    let tray_wnd = win32::find_child_window(0, "Shell_TrayWnd");
    if tray_wnd == 0 {
        return NativeToolResult::text_only("Error: cannot find Shell_TrayWnd".to_string());
    }

    let notify_wnd = win32::find_child_window(tray_wnd, "TrayNotifyWnd");
    if notify_wnd == 0 {
        return NativeToolResult::text_only("Error: cannot find TrayNotifyWnd".to_string());
    }

    // Use UI Automation to find buttons in the notification area
    let name_lower = name.to_lowercase();
    let result = std::thread::spawn(move || {
        ui_tools::find_ui_elements_all(notify_wnd, Some(&name_lower), Some("button"), 5)
    })
    .join()
    .unwrap_or_else(|_| Err("Thread panicked".to_string()));

    match result {
        Ok(elements) if !elements.is_empty() => {
            let el = &elements[0];
            let click_result = super::tool_click_screen(&serde_json::json!({
                "x": el.cx,
                "y": el.cy,
                "delay_ms": 500,
                "screenshot": true
            }));
            NativeToolResult {
                text: format!(
                    "Clicked tray icon '{}' at ({}, {})",
                    el.name, el.cx, el.cy
                ),
                images: click_result.images,
            }
        }
        Ok(_) => {
            // Try searching without type filter (some tray icons aren't buttons)
            let name_lower2 = name.to_lowercase();
            let result2 = std::thread::spawn(move || {
                ui_tools::find_ui_elements_all(notify_wnd, Some(&name_lower2), None, 5)
            })
            .join()
            .unwrap_or_else(|_| Err("Thread panicked".to_string()));

            match result2 {
                Ok(els) if !els.is_empty() => {
                    let el = &els[0];
                    let click_result = super::tool_click_screen(&serde_json::json!({
                        "x": el.cx, "y": el.cy, "delay_ms": 500, "screenshot": true
                    }));
                    NativeToolResult {
                        text: format!("Clicked tray element '{}' at ({}, {})", el.name, el.cx, el.cy),
                        images: click_result.images,
                    }
                }
                Ok(_) => NativeToolResult::text_only(format!("Tray icon matching '{}' not found", name)),
                Err(e) => NativeToolResult::text_only(format!("Error: {e}")),
            }
        }
        Err(e) => NativeToolResult::text_only(format!("Error: {e}")),
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_click_tray_icon(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: click_tray_icon is not available on this platform".to_string())
}

/// Watch for window changes (new, closed, title changes) with timeout.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_watch_window(args: &Value) -> NativeToolResult {
    #[cfg(windows)]
    use super::win32;
    #[cfg(target_os = "macos")]
    use super::macos as win32;
    #[cfg(target_os = "linux")]
    use super::linux as win32;

    let timeout_ms = args
        .get("timeout_ms")
        .and_then(parse_int)
        .unwrap_or(10000) as u64;
    let filter = args.get("filter").and_then(|v| v.as_str());
    let poll_ms = args.get("poll_ms").and_then(parse_int).unwrap_or(500) as u64;

    // Snapshot current windows
    let initial = win32::enumerate_windows();
    let initial_set: std::collections::HashMap<String, String> = initial
        .iter()
        .map(|w| (format!("{}:{}", w.process_name, w.class_name), w.title.clone()))
        .collect();

    let start = std::time::Instant::now();
    loop {
        let elapsed = start.elapsed().as_millis() as u64;
        if elapsed >= timeout_ms {
            return NativeToolResult::text_only(format!(
                "No window changes detected within {}ms",
                timeout_ms
            ));
        }

        std::thread::sleep(std::time::Duration::from_millis(poll_ms));

        let current = win32::enumerate_windows();
        let current_set: std::collections::HashMap<String, String> = current
            .iter()
            .map(|w| (format!("{}:{}", w.process_name, w.class_name), w.title.clone()))
            .collect();

        let mut changes = Vec::new();

        // Check for new windows
        for (key, title) in &current_set {
            if !initial_set.contains_key(key) {
                if filter.map_or(true, |f| title.to_lowercase().contains(&f.to_lowercase())) {
                    changes.push(format!("NEW: '{}'", title));
                }
            }
        }

        // Check for closed windows
        for (key, title) in &initial_set {
            if !current_set.contains_key(key) {
                if filter.map_or(true, |f| title.to_lowercase().contains(&f.to_lowercase())) {
                    changes.push(format!("CLOSED: '{}'", title));
                }
            }
        }

        // Check for title changes
        for (key, new_title) in &current_set {
            if let Some(old_title) = initial_set.get(key) {
                if old_title != new_title {
                    if filter.map_or(true, |f| {
                        new_title.to_lowercase().contains(&f.to_lowercase())
                            || old_title.to_lowercase().contains(&f.to_lowercase())
                    }) {
                        changes.push(format!("RENAMED: '{}' → '{}'", old_title, new_title));
                    }
                }
            }
        }

        if !changes.is_empty() {
            let screenshot = super::capture_post_action_screenshot(0);
            return NativeToolResult {
                text: format!(
                    "Window change(s) after {}ms:\n{}",
                    start.elapsed().as_millis(),
                    changes.join("\n")
                ),
                images: screenshot.images,
            };
        }
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_watch_window(_args: &Value) -> NativeToolResult {
    NativeToolResult::text_only("Error: watch_window is not available on this platform".to_string())
}

/// Send a desktop notification (toast on Windows, notify-send on Linux, osascript on macOS).
pub fn tool_send_notification(args: &Value) -> NativeToolResult {
    let title = args
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("Claude Code");
    let message = match args.get("message").and_then(|v| v.as_str()) {
        Some(m) => m,
        None => return NativeToolResult::text_only("Error: 'message' is required".to_string()),
    };

    #[cfg(windows)]
    {
        // Use PowerShell to show a Windows balloon notification
        let ps_script = format!(
            r#"Add-Type -AssemblyName System.Windows.Forms;
$n = New-Object System.Windows.Forms.NotifyIcon;
$n.Icon = [System.Drawing.SystemIcons]::Information;
$n.BalloonTipIcon = 'Info';
$n.BalloonTipTitle = '{}';
$n.BalloonTipText = '{}';
$n.Visible = $true;
$n.ShowBalloonTip(5000);
Start-Sleep -Seconds 6;
$n.Dispose();"#,
            title.replace('\'', "''").replace('`', "``"),
            message.replace('\'', "''").replace('`', "``"),
        );

        use std::process::{Command, Stdio};
        use std::os::windows::process::CommandExt;

        let mut cmd = Command::new("powershell");
        cmd.args(["-NoProfile", "-NonInteractive", "-Command", &ps_script])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(0x08000000); // CREATE_NO_WINDOW

        let result = cmd.spawn();

        match result {
            Ok(_child) => {
                // Don't wait — notification shows asynchronously
                NativeToolResult::text_only(format!(
                    "Notification sent: [{}] {}",
                    title, message
                ))
            }
            Err(e) => NativeToolResult::text_only(format!("Error sending notification: {e}")),
        }
    }

    #[cfg(target_os = "macos")]
    {
        use std::process::{Command, Stdio};
        let result = Command::new("osascript")
            .args([
                "-e",
                &format!(
                    "display notification \"{}\" with title \"{}\"",
                    message.replace('"', "\\\""),
                    title.replace('"', "\\\"")
                ),
            ])
            .stdin(Stdio::null())
            .output();

        match result {
            Ok(_) => NativeToolResult::text_only(format!("Notification sent: [{}] {}", title, message)),
            Err(e) => NativeToolResult::text_only(format!("Error sending notification: {e}")),
        }
    }

    #[cfg(target_os = "linux")]
    {
        use std::process::{Command, Stdio};
        let result = Command::new("notify-send")
            .args([title, message])
            .stdin(Stdio::null())
            .output();

        match result {
            Ok(_) => NativeToolResult::text_only(format!("Notification sent: [{}] {}", title, message)),
            Err(e) => NativeToolResult::text_only(format!("Error sending notification: {e}")),
        }
    }

    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        NativeToolResult::text_only("Error: send_notification is not available on this platform".to_string())
    }
}
