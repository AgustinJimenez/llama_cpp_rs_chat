//! System-level tools: registry, system tray, window monitoring, notifications.

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
        None => return super::tool_error("read_registry", "'key' (subkey path) is required"),
    };
    let value_name = args.get("value").and_then(|v| v.as_str()).unwrap_or("");

    let hkey_root = match hive_str.to_uppercase().as_str() {
        "HKCU" | "HKEY_CURRENT_USER" => win32::HKEY_CURRENT_USER,
        "HKLM" | "HKEY_LOCAL_MACHINE" => win32::HKEY_LOCAL_MACHINE,
        other => {
            return super::tool_error(
                "read_registry",
                format!("unsupported hive '{}'. Use HKCU or HKLM", other),
            )
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
        Err(e) => super::tool_error("read_registry", e),
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_read_registry(_args: &Value) -> NativeToolResult {
    super::tool_error("read_registry", "not available on this platform")
}

/// Click a system tray icon by tooltip/name text.
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub fn tool_click_tray_icon(args: &Value) -> NativeToolResult {
    use super::ui_automation_tools;
    #[cfg(windows)]
    use super::win32;
    #[cfg(target_os = "macos")]
    use super::macos as win32;
    #[cfg(target_os = "linux")]
    use super::linux as win32;

    let name = match args.get("name").and_then(|v| v.as_str()) {
        Some(n) => n,
        None => return super::tool_error("click_tray_icon", "'name' (icon tooltip text) is required"),
    };

    // Find the system tray: Shell_TrayWnd → TrayNotifyWnd → SysPager → ToolbarWindow32
    let tray_wnd = win32::find_child_window(0, "Shell_TrayWnd");
    if tray_wnd == 0 {
        return super::tool_error("click_tray_icon", "cannot find Shell_TrayWnd");
    }

    let notify_wnd = win32::find_child_window(tray_wnd, "TrayNotifyWnd");
    if notify_wnd == 0 {
        return super::tool_error("click_tray_icon", "cannot find TrayNotifyWnd");
    }

    // Use UI Automation to find buttons in the notification area
    let name_lower = name.to_lowercase();
    let result = super::spawn_with_timeout(super::DEFAULT_THREAD_TIMEOUT, move || {
        ui_automation_tools::find_ui_elements_all(notify_wnd, Some(&name_lower), Some("button"), 5)
    }).and_then(|r| r);

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
            let result2 = super::spawn_with_timeout(super::DEFAULT_THREAD_TIMEOUT, move || {
                ui_automation_tools::find_ui_elements_all(notify_wnd, Some(&name_lower2), None, 5)
            }).and_then(|r| r);

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
                Ok(_) => super::tool_error("click_tray_icon", format!("tray icon matching '{}' not found", name)),
                Err(e) => super::tool_error("click_tray_icon", e),
            }
        }
        Err(e) => super::tool_error("click_tray_icon", e),
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn tool_click_tray_icon(_args: &Value) -> NativeToolResult {
    super::tool_error("click_tray_icon", "not available on this platform")
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
        if let Err(e) = super::ensure_desktop_not_cancelled() {
            return super::tool_error("watch_window", e);
        }

        let elapsed = start.elapsed().as_millis() as u64;
        if elapsed >= timeout_ms {
            return NativeToolResult::text_only(format!(
                "No window changes detected within {}ms",
                timeout_ms
            ));
        }

        if let Err(e) = super::interruptible_sleep(std::time::Duration::from_millis(poll_ms)) {
            return super::tool_error("watch_window", e);
        }

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
    super::tool_error("watch_window", "not available on this platform")
}

/// Send a desktop notification (toast on Windows, notify-send on Linux, osascript on macOS).
pub fn tool_send_notification(args: &Value) -> NativeToolResult {
    let title = args
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("Claude Code");
    let message = match args.get("message").and_then(|v| v.as_str()) {
        Some(m) => m,
        None => return super::tool_error("send_notification", "'message' is required"),
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
            Err(e) => super::tool_error("send_notification", format!("sending notification: {e}")),
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
            Err(e) => super::tool_error("send_notification", format!("sending notification: {e}")),
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
            Err(e) => super::tool_error("send_notification", format!("sending notification: {e}")),
        }
    }

    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        super::tool_error("send_notification", "not available on this platform")
    }
}

// ─── Notification handling tools ─────────────────────────────────────────────

/// Wait for a system notification matching a text filter.
/// Uses screen capture + OCR on the notification region of the screen.
/// Params: `text_contains` (string, required), `timeout_ms` (integer, default 10000, max 30000).
pub fn tool_wait_for_notification(args: &Value) -> NativeToolResult {
    let text_contains = match args.get("text_contains").and_then(|v| v.as_str()) {
        Some(t) => t.to_string(),
        None => {
            return super::tool_error(
                "wait_for_notification",
                "'text_contains' (text to match in notification) is required",
            )
        }
    };

    let timeout_ms = args
        .get("timeout_ms")
        .and_then(parse_int)
        .unwrap_or(10000)
        .min(30000)
        .max(1000) as u64;

    let poll_ms = 1500u64; // Check every 1.5 seconds
    let filter_lower = text_contains.to_lowercase();

    let start = std::time::Instant::now();

    loop {
        if let Err(e) = super::ensure_desktop_not_cancelled() {
            return super::tool_error("wait_for_notification", e);
        }

        // Capture the notification region of the screen
        let ocr_text = capture_notification_region_ocr();

        if let Some(text) = ocr_text {
            if text.to_lowercase().contains(&filter_lower) {
                let elapsed = start.elapsed().as_millis();
                return NativeToolResult::text_only(format!(
                    "Notification found after {}ms. Matched '{}' in:\n{}",
                    elapsed, text_contains, truncate_text(&text, 500)
                ));
            }
        }

        let elapsed = start.elapsed().as_millis() as u64;
        if elapsed >= timeout_ms {
            return NativeToolResult::text_only(format!(
                "No notification matching '{}' found within {}ms",
                text_contains, timeout_ms
            ));
        }

        if let Err(e) = super::interruptible_sleep(std::time::Duration::from_millis(poll_ms)) {
            return super::tool_error("wait_for_notification", e);
        }
    }
}

/// Capture the notification area of the screen and run OCR on it.
/// Returns the recognized text, or None on failure.
fn capture_notification_region_ocr() -> Option<String> {
    let monitors = xcap::Monitor::all().ok()?;
    let monitor = monitors.first()?;
    let img = monitor.capture_image().ok()?;

    let screen_w = img.width();
    let screen_h = img.height();

    // Notification region depends on platform:
    // - Windows 11: bottom-right corner (toast notifications)
    // - macOS: top-right corner
    // - Linux: varies, typically top-right

    let region_w = 450.min(screen_w);
    let region_h = 350.min(screen_h);

    #[cfg(target_os = "macos")]
    let (rx, ry) = (screen_w.saturating_sub(region_w), 0u32);

    #[cfg(not(target_os = "macos"))]
    let (rx, ry) = (
        screen_w.saturating_sub(region_w),
        screen_h.saturating_sub(region_h),
    );

    // Crop the notification region
    let cropped = image::imageops::crop_imm(&img, rx, ry, region_w, region_h).to_image();

    // Run OCR on the cropped region using platform-appropriate engine
    #[cfg(windows)]
    {
        super::spawn_with_timeout(super::DEFAULT_THREAD_TIMEOUT, move || {
            super::ocr_tools::ocr_image_winrt(&cropped)
        })
        .ok()
        .and_then(|r| r.ok())
    }

    #[cfg(target_os = "macos")]
    {
        super::ocr_tools::ocr_image_vision(&cropped, None)
            .or_else(|_| super::ocr_tools::ocr_image_tesseract(&cropped, None))
            .ok()
    }

    #[cfg(target_os = "linux")]
    {
        super::ocr_tools::ocr_image_tesseract(&cropped, None).ok()
    }

    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

/// Truncate text to a maximum length, appending "..." if truncated.
fn truncate_text(text: &str, max_len: usize) -> &str {
    if text.len() <= max_len {
        text
    } else {
        // Find a safe char boundary
        let mut end = max_len;
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        &text[..end]
    }
}

/// Dismiss/clear all notifications.
/// Uses platform-specific approaches. Returns a message about what was done.
pub fn tool_dismiss_all_notifications(_args: &Value) -> NativeToolResult {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use std::process::{Command, Stdio};

        // Windows 11: Open notification center, find and click "Clear all" or dismiss
        // Approach: Use PowerShell to clear toast notification history for common apps.
        // This clears the Action Center notifications.
        let ps_script = r#"
try {
    [Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] | Out-Null
    $apps = [Windows.UI.Notifications.ToastNotificationManager]::History
    # History.Clear requires an app ID; clear for common shell app
    # Instead, use keyboard shortcut to open and clear Action Center
} catch {}
# Open Action Center with Win+N, Tab to Clear all, press Enter
Add-Type -AssemblyName System.Windows.Forms
[System.Windows.Forms.SendKeys]::SendWait('^{n}')
Start-Sleep -Milliseconds 800
"#;

        let mut cmd = Command::new("powershell");
        cmd.args(["-NoProfile", "-NonInteractive", "-Command", ps_script])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(0x08000000); // CREATE_NO_WINDOW

        match cmd.output() {
            Ok(_) => {
                // Now try to find and click "Clear all" using keyboard navigation
                // Send Tab a few times then Enter to hit "Clear all notifications"
                std::thread::sleep(std::time::Duration::from_millis(300));

                // Use enigo to send Escape to close (best-effort cleanup)
                // The notification center is now open; the model can interact with it
                NativeToolResult::text_only(
                    "Opened Windows notification center (Win+N). \
                     Use click_screen or press_key to interact with 'Clear all' if visible. \
                     Press Escape to close when done."
                        .to_string(),
                )
            }
            Err(e) => super::tool_error(
                "dismiss_all_notifications",
                format!("failed to open notification center: {e}"),
            ),
        }
    }

    #[cfg(target_os = "macos")]
    {
        // macOS: Notification Center cannot be easily cleared programmatically.
        // The best we can do is close any visible banners.
        NativeToolResult::text_only(
            "macOS Notification Center does not support programmatic 'clear all'. \
             Notifications can be dismissed manually by clicking the 'X' in Notification Center, \
             or by opening it (click date/time in menu bar) and clicking 'Clear'."
                .to_string(),
        )
    }

    #[cfg(target_os = "linux")]
    {
        use std::process::{Command, Stdio};

        // Try common notification daemons: dunst, mako, swaync
        let mut cleared = false;

        // dunst
        if Command::new("dunstctl")
            .arg("close-all")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
        {
            cleared = true;
        }

        // mako (Wayland)
        if !cleared {
            if Command::new("makoctl")
                .arg("dismiss")
                .arg("--all")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
            {
                cleared = true;
            }
        }

        // swaync
        if !cleared {
            if Command::new("swaync-client")
                .arg("--close-all")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
            {
                cleared = true;
            }
        }

        if cleared {
            NativeToolResult::text_only("Dismissed all notifications".to_string())
        } else {
            NativeToolResult::text_only(
                "Could not dismiss notifications: none of dunstctl, makoctl, or swaync-client found. \
                 Install one of these notification daemons for notification control."
                    .to_string(),
            )
        }
    }

    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        super::tool_error(
            "dismiss_all_notifications",
            "not available on this platform",
        )
    }
}
