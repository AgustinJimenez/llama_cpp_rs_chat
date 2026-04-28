//! Audio control tools: get/set system volume and mute state.
//!
//! Cross-platform support:
//! - **Windows**: PowerShell COM interop with `IAudioEndpointVolume`
//! - **macOS**: AppleScript via `osascript`
//! - **Linux**: `amixer` CLI

use serde_json::Value;

use super::NativeToolResult;
use super::{parse_bool, parse_int, tool_error};

// ---------------------------------------------------------------------------
// Windows: PowerShell + Core Audio COM helper
// ---------------------------------------------------------------------------

/// PowerShell preamble that creates a `$v` variable bound to the default
/// audio endpoint's `IAudioEndpointVolume` interface.  Reused by get, set,
/// and mute operations.
#[cfg(windows)]
const AUDIO_PS_PREAMBLE: &str = r#"
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;

[Guid("5CDF2C82-841E-4546-9722-0CF74078229A"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IAudioEndpointVolume {
    int _0(); int _1(); int _2(); int _3(); int _4(); int _5(); int _6(); int _7();
    int SetMasterVolumeLevelScalar(float level, System.Guid ctx);
    int _9();
    int GetMasterVolumeLevelScalar(out float level);
    int SetMute([MarshalAs(UnmanagedType.Bool)] bool mute, System.Guid ctx);
    int GetMute([MarshalAs(UnmanagedType.Bool)] out bool mute);
}

[Guid("D666063F-1587-4E43-81F1-B948E807363F"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IMMDevice {
    int Activate(ref System.Guid iid, int ctx, System.IntPtr p,
        [MarshalAs(UnmanagedType.Interface)] out IAudioEndpointVolume aev);
}

[Guid("A95664D2-9614-4F35-A746-DE8DB63617E6"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IMMDeviceEnumerator {
    int GetDefaultAudioEndpoint(int flow, int role,
        [MarshalAs(UnmanagedType.Interface)] out IMMDevice device);
}

[ComImport, Guid("BCDE0395-E52F-467C-8E3D-C4579291692E")]
class MMDeviceEnumerator {}
"@

$enum = New-Object MMDeviceEnumerator
$device = $null
$enum.GetDefaultAudioEndpoint(0, 1, [ref]$device) | Out-Null
$iid = [Guid]"5CDF2C82-841E-4546-9722-0CF74078229A"
$v = $null
$device.Activate([ref]$iid, 1, [IntPtr]::Zero, [ref]$v) | Out-Null
"#;

/// Execute a PowerShell snippet that builds on [`AUDIO_PS_PREAMBLE`].
#[cfg(windows)]
fn run_audio_ps(action: &str) -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    const CREATE_NO_WINDOW: u32 = 0x08000000;

    let script = format!("{}\n{}", AUDIO_PS_PREAMBLE, action);
    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("PowerShell launch failed: {e}"))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if stderr.is_empty() {
            format!("PowerShell exited with {}", output.status)
        } else {
            stderr
        })
    }
}

// ---------------------------------------------------------------------------
// macOS: osascript helper (mirrors macos.rs pattern)
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
fn run_osascript(script: &str) -> Result<String, String> {
    use std::process::Command;
    let output = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .map_err(|e| format!("Failed to run osascript: {e}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(format!("osascript error: {stderr}"))
    }
}

// ---------------------------------------------------------------------------
// Linux: amixer helper (mirrors linux.rs run_cmd pattern)
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
fn run_cmd(prog: &str, args: &[&str]) -> Result<String, String> {
    use std::process::Command;
    let output = Command::new(prog)
        .args(args)
        .output()
        .map_err(|e| format!("Failed to run {prog}: {e}. Is it installed?"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(format!("{prog} error: {stderr}"))
    }
}

// ---------------------------------------------------------------------------
// Tool: get_system_volume
// ---------------------------------------------------------------------------

/// Return the current system volume (0-100) and mute state.
///
/// No required parameters.
/// Output format: `"Volume: <N>%, Muted: <true|false>"`
pub fn tool_get_system_volume(_args: &Value) -> NativeToolResult {
    match get_system_volume_impl() {
        Ok((volume, muted)) => {
            NativeToolResult::text_only(format!("Volume: {volume}%, Muted: {muted}"))
        }
        Err(e) => tool_error("get_system_volume", e),
    }
}

#[cfg(windows)]
fn get_system_volume_impl() -> Result<(u32, bool), String> {
    let raw = run_audio_ps(
        r#"$l = 0.0; $v.GetMasterVolumeLevelScalar([ref]$l) | Out-Null; $m = $false; $v.GetMute([ref]$m) | Out-Null; Write-Output "$([math]::Round($l * 100)),$m""#,
    )?;
    // Expected output: "75,False" or "0,True"
    let parts: Vec<&str> = raw.splitn(2, ',').collect();
    if parts.len() != 2 {
        return Err(format!("Unexpected PowerShell output: {raw}"));
    }
    let volume: u32 = parts[0]
        .trim()
        .parse()
        .map_err(|_| format!("Cannot parse volume from '{}'", parts[0]))?;
    let muted = parts[1].trim().eq_ignore_ascii_case("true");
    Ok((volume, muted))
}

#[cfg(target_os = "macos")]
fn get_system_volume_impl() -> Result<(u32, bool), String> {
    let vol_str = run_osascript("output volume of (get volume settings)")?;
    let muted_str = run_osascript("output muted of (get volume settings)")?;
    let volume: u32 = vol_str
        .trim()
        .parse()
        .map_err(|_| format!("Cannot parse volume from '{vol_str}'"))?;
    let muted = muted_str.trim().eq_ignore_ascii_case("true");
    Ok((volume, muted))
}

#[cfg(target_os = "linux")]
fn get_system_volume_impl() -> Result<(u32, bool), String> {
    let output = run_cmd("amixer", &["get", "Master"])?;
    // Parse lines like: "  Front Left: Playback 32768 [50%] [on]"
    let mut volume: Option<u32> = None;
    let mut muted = false;
    for line in output.lines() {
        // Extract percentage: "[50%]"
        if let Some(start) = line.find('[') {
            if let Some(end) = line[start..].find('%') {
                if let Ok(v) = line[start + 1..start + end].parse::<u32>() {
                    volume = Some(v);
                }
            }
        }
        // Extract mute state: "[off]" or "[on]"
        if line.contains("[off]") {
            muted = true;
        }
    }
    let volume = volume.ok_or_else(|| "Could not parse volume from amixer output".to_string())?;
    Ok((volume, muted))
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
fn get_system_volume_impl() -> Result<(u32, bool), String> {
    Err("get_system_volume is not supported on this platform".to_string())
}

// ---------------------------------------------------------------------------
// Tool: set_system_volume
// ---------------------------------------------------------------------------

/// Set the system volume to a value between 0 and 100.
///
/// Required parameter: `level` (integer, 0-100).
pub fn tool_set_system_volume(args: &Value) -> NativeToolResult {
    let level = match args.get("level").and_then(parse_int) {
        Some(v) => v as i32,
        None => {
            return tool_error(
                "set_system_volume",
                "'level' parameter is required (integer 0-100)",
            );
        }
    };
    if !(0..=100).contains(&level) {
        return tool_error("set_system_volume", "'level' must be between 0 and 100");
    }
    match set_system_volume_impl(level as u32) {
        Ok(()) => NativeToolResult::text_only(format!("Volume set to {level}%")),
        Err(e) => tool_error("set_system_volume", e),
    }
}

#[cfg(windows)]
fn set_system_volume_impl(level: u32) -> Result<(), String> {
    let action = format!(
        "$v.SetMasterVolumeLevelScalar({level} / 100.0, [Guid]::Empty) | Out-Null; Write-Output 'ok'"
    );
    let result = run_audio_ps(&action)?;
    if result.contains("ok") {
        Ok(())
    } else {
        Err(format!("Unexpected output: {result}"))
    }
}

#[cfg(target_os = "macos")]
fn set_system_volume_impl(level: u32) -> Result<(), String> {
    run_osascript(&format!("set volume output volume {level}"))?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn set_system_volume_impl(level: u32) -> Result<(), String> {
    run_cmd("amixer", &["set", "Master", &format!("{level}%")])?;
    Ok(())
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
fn set_system_volume_impl(_level: u32) -> Result<(), String> {
    Err("set_system_volume is not supported on this platform".to_string())
}

// ---------------------------------------------------------------------------
// Tool: list_audio_devices
// ---------------------------------------------------------------------------

/// List available audio output devices on the system.
///
/// No required parameters.
pub fn tool_list_audio_devices(_args: &Value) -> NativeToolResult {
    match list_audio_devices_impl() {
        Ok(text) => NativeToolResult::text_only(text),
        Err(e) => tool_error("list_audio_devices", e),
    }
}

#[cfg(windows)]
fn list_audio_devices_impl() -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    const CREATE_NO_WINDOW: u32 = 0x08000000;

    let script = r#"
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;

[Guid("D666063F-1587-4E43-81F1-B948E807363F"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IMMDevice {
    int Activate(ref Guid iid, int ctx, IntPtr p, [MarshalAs(UnmanagedType.IUnknown)] out object aev);
    int OpenPropertyStore(int access, [MarshalAs(UnmanagedType.IUnknown)] out object props);
    int GetId([MarshalAs(UnmanagedType.LPWStr)] out string id);
    int GetState(out int state);
}

[Guid("0BD7A1BE-7A1A-44DB-8397-CC5392387B5E"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IMMDeviceCollection {
    int GetCount(out int count);
    int Item(int index, [MarshalAs(UnmanagedType.Interface)] out IMMDevice device);
}

[Guid("A95664D2-9614-4F35-A746-DE8DB63617E6"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IMMDeviceEnumerator {
    int GetDefaultAudioEndpoint(int flow, int role, [MarshalAs(UnmanagedType.Interface)] out IMMDevice device);
    int EnumAudioEndpoints(int flow, int stateMask, [MarshalAs(UnmanagedType.Interface)] out IMMDeviceCollection coll);
}

[ComImport, Guid("BCDE0395-E52F-467C-8E3D-C4579291692E")]
class MMDeviceEnumerator {}
"@

$enum = New-Object MMDeviceEnumerator
$coll = $null
# flow=0 (render/output), stateMask=1 (DEVICE_STATE_ACTIVE)
$enum.EnumAudioEndpoints(0, 1, [ref]$coll) | Out-Null
$count = 0
$coll.GetCount([ref]$count) | Out-Null

$defaultDev = $null
$enum.GetDefaultAudioEndpoint(0, 1, [ref]$defaultDev) | Out-Null
$defaultId = ""
if ($defaultDev) { $defaultDev.GetId([ref]$defaultId) | Out-Null }

$results = @()
for ($i = 0; $i -lt $count; $i++) {
    $dev = $null
    $coll.Item($i, [ref]$dev) | Out-Null
    $id = ""
    $dev.GetId([ref]$id) | Out-Null
    $state = 0
    $dev.GetState([ref]$state) | Out-Null
    $isDefault = if ($id -eq $defaultId) { " [DEFAULT]" } else { "" }
    $results += "$i. Device ID: $id State: $state$isDefault"
}
if ($results.Count -eq 0) { Write-Output "No active audio output devices found" }
else { $results | ForEach-Object { Write-Output $_ } }
"#;

    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("PowerShell launch failed: {e}"))?;

    if output.status.success() {
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if text.is_empty() {
            Ok("No active audio output devices found".to_string())
        } else {
            Ok(format!("Audio output devices:\n{text}"))
        }
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if stderr.is_empty() {
            format!("PowerShell exited with {}", output.status)
        } else {
            stderr
        })
    }
}

#[cfg(target_os = "macos")]
fn list_audio_devices_impl() -> Result<String, String> {
    // Use system_profiler to list audio devices
    use std::process::Command;
    let output = Command::new("system_profiler")
        .args(["SPAudioDataType", "-json"])
        .output()
        .map_err(|e| format!("system_profiler failed: {e}"))?;
    if output.status.success() {
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(format!("Audio devices:\n{text}"))
    } else {
        Err("system_profiler SPAudioDataType failed".to_string())
    }
}

#[cfg(target_os = "linux")]
fn list_audio_devices_impl() -> Result<String, String> {
    use std::process::Command;
    // Try pactl first (PulseAudio/PipeWire), fall back to aplay
    let output = Command::new("pactl")
        .args(["list", "sinks", "short"])
        .output();
    if let Ok(out) = output {
        if out.status.success() {
            return Ok(format!("Audio sinks:\n{}", String::from_utf8_lossy(&out.stdout).trim()));
        }
    }
    let output = Command::new("aplay")
        .args(["-l"])
        .output()
        .map_err(|e| format!("aplay failed: {e}. Is alsa-utils installed?"))?;
    if output.status.success() {
        Ok(format!("Audio devices:\n{}", String::from_utf8_lossy(&output.stdout).trim()))
    } else {
        Err("aplay -l failed".to_string())
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
fn list_audio_devices_impl() -> Result<String, String> {
    Err("list_audio_devices is not supported on this platform".to_string())
}

// ---------------------------------------------------------------------------
// Tool: set_system_mute
// ---------------------------------------------------------------------------

/// Set the system mute state.
///
/// Required parameter: `muted` (boolean).
pub fn tool_set_system_mute(args: &Value) -> NativeToolResult {
    let muted_val = match args.get("muted") {
        Some(v) => v,
        None => {
            return tool_error(
                "set_system_mute",
                "'muted' parameter is required (boolean)",
            );
        }
    };
    let muted = parse_bool(muted_val, false);
    match set_system_mute_impl(muted) {
        Ok(()) => {
            let state = if muted { "muted" } else { "unmuted" };
            NativeToolResult::text_only(format!("System audio {state}"))
        }
        Err(e) => tool_error("set_system_mute", e),
    }
}

#[cfg(windows)]
fn set_system_mute_impl(muted: bool) -> Result<(), String> {
    let ps_bool = if muted { "$true" } else { "$false" };
    let action = format!(
        "$v.SetMute({ps_bool}, [Guid]::Empty) | Out-Null; Write-Output 'ok'"
    );
    let result = run_audio_ps(&action)?;
    if result.contains("ok") {
        Ok(())
    } else {
        Err(format!("Unexpected output: {result}"))
    }
}

#[cfg(target_os = "macos")]
fn set_system_mute_impl(muted: bool) -> Result<(), String> {
    let val = if muted { "true" } else { "false" };
    run_osascript(&format!("set volume output muted {val}"))?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn set_system_mute_impl(muted: bool) -> Result<(), String> {
    let state = if muted { "mute" } else { "unmute" };
    run_cmd("amixer", &["set", "Master", state])?;
    Ok(())
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
fn set_system_mute_impl(_muted: bool) -> Result<(), String> {
    Err("set_system_mute is not supported on this platform".to_string())
}
