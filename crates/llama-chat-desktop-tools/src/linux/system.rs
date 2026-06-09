use super::{run_cmd, DWORD, HKEY, HWND};
use enigo::Key;
use std::process::Command;

pub fn get_cursor_position() -> (i32, i32) {
    if let Ok(output) = run_cmd("xdotool", &["getmouselocation", "--shell"]) {
        let mut x = 0i32;
        let mut y = 0i32;
        for line in output.lines() {
            if let Some(val) = line.strip_prefix("X=") {
                x = val.parse().unwrap_or(0);
            }
            if let Some(val) = line.strip_prefix("Y=") {
                y = val.parse().unwrap_or(0);
            }
        }
        (x, y)
    } else {
        (0, 0)
    }
}

pub fn get_pixel_color(x: i32, y: i32) -> Result<(u8, u8, u8), String> {
    let monitors = xcap::Monitor::all().map_err(|e| format!("xcap error: {e}"))?;
    let monitor = monitors.first().ok_or("No monitors")?;
    let img = monitor
        .capture_image()
        .map_err(|e| format!("capture error: {e}"))?;
    let mx = (x - monitor.x().unwrap_or(0)) as u32;
    let my = (y - monitor.y().unwrap_or(0)) as u32;
    if mx < img.width() && my < img.height() {
        let pixel = img.get_pixel(mx, my);
        Ok((pixel[0], pixel[1], pixel[2]))
    } else {
        Err(format!("Coordinates ({x}, {y}) out of screen bounds"))
    }
}

pub fn read_clipboard() -> Result<String, String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|e| format!("Clipboard error: {e}"))?;
    clipboard
        .get_text()
        .map_err(|e| format!("Clipboard read error: {e}"))
}

pub fn write_clipboard(text: &str) -> Result<(), String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|e| format!("Clipboard error: {e}"))?;
    clipboard
        .set_text(text.to_string())
        .map_err(|e| format!("Clipboard write error: {e}"))
}

pub fn read_clipboard_files() -> Result<Vec<String>, String> {
    Err("Clipboard file reading not supported on Linux".to_string())
}

pub fn get_clipboard_formats() -> Vec<&'static str> {
    let mut formats = Vec::new();
    if let Ok(mut cb) = arboard::Clipboard::new() {
        if cb.get_text().is_ok() {
            formats.push("text");
        }
        if cb.get_image().is_ok() {
            formats.push("image");
        }
    }
    formats
}

pub fn shell_execute(file: &str, args: Option<&str>) -> Result<(), String> {
    let mut cmd = Command::new("xdg-open");
    cmd.arg(file);
    if let Some(a) = args {
        let mut cmd2 = Command::new(file);
        for part in a.split_whitespace() {
            cmd2.arg(part);
        }
        cmd2.spawn().map_err(|e| format!("exec failed: {e}"))?;
        return Ok(());
    }
    cmd.spawn().map_err(|e| format!("xdg-open failed: {e}"))?;
    Ok(())
}

pub fn enumerate_processes() -> Result<Vec<(DWORD, String)>, String> {
    use sysinfo::System;
    let mut sys = System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    let mut result = Vec::new();
    for (pid, proc_) in sys.processes() {
        result.push((pid.as_u32(), proc_.name().to_string_lossy().to_string()));
    }
    Ok(result)
}

pub fn terminate_process(pid: DWORD) -> Result<(), String> {
    use sysinfo::System;
    let mut sys = System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    let spid = sysinfo::Pid::from_u32(pid);
    if let Some(proc_) = sys.process(spid) {
        if proc_.kill() {
            Ok(())
        } else {
            Err(format!("Failed to kill PID {pid}"))
        }
    } else {
        Err(format!("Process {pid} not found"))
    }
}

pub fn get_process_resource_info(pid: DWORD) -> Result<(usize, u64, u64), String> {
    use sysinfo::System;
    let mut sys = System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    let spid = sysinfo::Pid::from_u32(pid);
    if let Some(proc_) = sys.process(spid) {
        let mem = proc_.memory() as usize;
        let cpu_time = proc_.run_time() * 1000;
        Ok((mem, 0, cpu_time))
    } else {
        Err(format!("Process {pid} not found"))
    }
}

pub fn key_to_vk(key: &Key) -> Option<u32> {
    match key {
        Key::Return => Some(0x0D),
        Key::Tab => Some(0x09),
        Key::Escape => Some(0x1B),
        Key::Backspace => Some(0x08),
        Key::Delete => Some(0x2E),
        Key::Space => Some(0x20),
        Key::UpArrow => Some(0x26),
        Key::DownArrow => Some(0x28),
        Key::LeftArrow => Some(0x25),
        Key::RightArrow => Some(0x27),
        Key::Home => Some(0x24),
        Key::End => Some(0x23),
        Key::PageUp => Some(0x21),
        Key::PageDown => Some(0x22),
        Key::Control => Some(0x11),
        Key::Alt => Some(0x12),
        Key::Shift => Some(0x10),
        Key::Meta => Some(0x5B),
        Key::CapsLock => Some(0x14),
        Key::F1 => Some(0x70),
        Key::F2 => Some(0x71),
        Key::F3 => Some(0x72),
        Key::F4 => Some(0x73),
        Key::F5 => Some(0x74),
        Key::F6 => Some(0x75),
        Key::F7 => Some(0x76),
        Key::F8 => Some(0x77),
        Key::F9 => Some(0x78),
        Key::F10 => Some(0x79),
        Key::F11 => Some(0x7A),
        Key::F12 => Some(0x7B),
        Key::Unicode(c) => Some(*c as u32),
        _ => None,
    }
}

pub fn get_window_class_name(_hwnd: HWND) -> String {
    String::new()
}

pub fn get_system_dpi_scale() -> f64 {
    if let Ok(monitors) = xcap::Monitor::all() {
        if let Some(m) = monitors.first() {
            return m.scale_factor().unwrap_or(1.0) as f64;
        }
    }
    1.0
}

pub fn set_window_opacity(hwnd: HWND, alpha: u8) -> Result<(), String> {
    let hex = format!("0x{:08x}", hwnd as u64);
    let opacity = (alpha as u64) * 0x01010101;
    let opacity_str = format!("{opacity}");
    run_cmd(
        "xprop",
        &[
            "-id",
            &hex,
            "-f",
            "_NET_WM_WINDOW_OPACITY",
            "32c",
            "-set",
            "_NET_WM_WINDOW_OPACITY",
            &opacity_str,
        ],
    )?;
    Ok(())
}

pub fn read_registry_value(
    _hkey_root: HKEY,
    _subkey: &str,
    _value_name: &str,
) -> Result<String, String> {
    Err("Registry is Windows-only. Use config files on Linux.".to_string())
}

pub fn is_process_alive(pid: DWORD) -> bool {
    if let Ok(procs) = enumerate_processes() {
        procs.iter().any(|(p, _)| *p == pid)
    } else {
        false
    }
}
