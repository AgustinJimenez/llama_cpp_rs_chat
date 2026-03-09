//! Status overlay tools: persistent on-screen bar showing automation progress.
//!
//! The overlay is a PowerShell WinForms window that polls a control file for text updates.
//! Lifecycle: show_status_overlay (spawn) → update_status_overlay (write file) → hide_status_overlay (write __EXIT__).

use serde_json::Value;
use std::io::Write;
use std::path::PathBuf;
use std::process::Child;
use std::sync::Mutex;

use super::NativeToolResult;

// ─── State persistence ───────────────────────────────────────────────────────

struct OverlayState {
    child: Child,
    control_file: PathBuf,
}

lazy_static::lazy_static! {
    /// Holds the PowerShell overlay child process and control file path.
    static ref OVERLAY: Mutex<Option<OverlayState>> = Mutex::new(None);
}

/// The PowerShell WinForms script. Takes $ControlFile variable, polls it for text
/// updates every 100ms. "__EXIT__" or file deletion closes the window.
/// Hidden from Alt+Tab (WS_EX_TOOLWINDOW). Has a clickable X button so the user
/// can dismiss it manually. Does not steal focus on show.
#[cfg(windows)]
const OVERLAY_PS_SCRIPT: &str = r#"
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

if (-not $ControlFile -or -not (Test-Path $ControlFile)) { exit 1 }
$text = Get-Content $ControlFile -Raw -ErrorAction SilentlyContinue
if (-not $text) { $text = "..." }

$form = New-Object System.Windows.Forms.Form
$form.FormBorderStyle = 'None'
$form.StartPosition = 'Manual'
$form.TopMost = $true
$form.ShowInTaskbar = $false
$form.BackColor = [System.Drawing.Color]::FromArgb(25, 25, 30)
$form.Opacity = 0.88
$form.Height = 32

$screen = [System.Windows.Forms.Screen]::PrimaryScreen.WorkingArea
$form.Width = $screen.Width
$form.Left = $screen.Left
if ($Position -eq 'bottom') {
    $form.Top = $screen.Bottom - $form.Height
} else {
    $form.Top = $screen.Top
}

# Hidden from Alt+Tab but NOT click-through (user can click X)
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class W32 {
    [DllImport("user32.dll")] public static extern int GetWindowLong(IntPtr h, int i);
    [DllImport("user32.dll")] public static extern int SetWindowLong(IntPtr h, int i, int v);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
    public const int GWL_EXSTYLE = -20;
    public const int WS_EX_TOOLWINDOW = 0x80;
    public const int WS_EX_NOACTIVATE = 0x08000000;
    public const int SW_SHOWNOACTIVATE = 4;
}
"@

$form.Add_Shown({
    $style = [W32]::GetWindowLong($form.Handle, [W32]::GWL_EXSTYLE)
    $style = $style -bor [W32]::WS_EX_TOOLWINDOW -bor [W32]::WS_EX_NOACTIVATE
    [W32]::SetWindowLong($form.Handle, [W32]::GWL_EXSTYLE, $style) | Out-Null
})

# Status label (fills most of the bar)
$label = New-Object System.Windows.Forms.Label
$label.Text = $text.Trim()
$label.ForeColor = [System.Drawing.Color]::White
$label.BackColor = [System.Drawing.Color]::Transparent
$label.Font = New-Object System.Drawing.Font("Segoe UI", 10)
$label.AutoSize = $false
$label.Location = New-Object System.Drawing.Point(10, 0)
$label.Size = New-Object System.Drawing.Size(($form.Width - 50), $form.Height)
$label.TextAlign = 'MiddleLeft'
$form.Controls.Add($label)

# Close button (X) on the right — clickable!
$closeBtn = New-Object System.Windows.Forms.Label
$closeBtn.Text = [char]0x2715  # Unicode X mark
$closeBtn.ForeColor = [System.Drawing.Color]::FromArgb(180, 180, 180)
$closeBtn.BackColor = [System.Drawing.Color]::Transparent
$closeBtn.Font = New-Object System.Drawing.Font("Segoe UI", 12)
$closeBtn.Size = New-Object System.Drawing.Size(36, $form.Height)
$closeBtn.Location = New-Object System.Drawing.Point(($form.Width - 40), 0)
$closeBtn.TextAlign = 'MiddleCenter'
$closeBtn.Cursor = [System.Windows.Forms.Cursors]::Hand
$closeBtn.Add_Click({ $form.Close() })
$closeBtn.Add_MouseEnter({ $closeBtn.ForeColor = [System.Drawing.Color]::White })
$closeBtn.Add_MouseLeave({ $closeBtn.ForeColor = [System.Drawing.Color]::FromArgb(180, 180, 180) })
$form.Controls.Add($closeBtn)
$closeBtn.BringToFront()

# Don't steal focus when showing
$form.Add_Load({
    [W32]::ShowWindow($form.Handle, [W32]::SW_SHOWNOACTIVATE) | Out-Null
})

$script:lastText = $text
$timer = New-Object System.Windows.Forms.Timer
$timer.Interval = 100
$timer.Add_Tick({
    if (-not (Test-Path $ControlFile)) {
        $timer.Stop(); $form.Close(); return
    }
    $current = Get-Content $ControlFile -Raw -ErrorAction SilentlyContinue
    if ($null -eq $current -or $current.Trim() -eq "__EXIT__") {
        $timer.Stop(); $form.Close(); return
    }
    $trimmed = $current.Trim()
    if ($trimmed -ne $script:lastText) {
        $label.Text = $trimmed
        $script:lastText = $trimmed
    }
})
$timer.Start()

$form.Add_FormClosed({
    $timer.Stop(); $timer.Dispose()
    if (Test-Path $ControlFile) { Remove-Item $ControlFile -Force -ErrorAction SilentlyContinue }
})

[System.Windows.Forms.Application]::Run($form)
"#;

// ─── Tool functions ──────────────────────────────────────────────────────────

fn control_file_path() -> PathBuf {
    std::env::temp_dir().join("claude_overlay_ctrl.txt")
}

/// Show a persistent status overlay bar on screen.
/// Args: { text: String, position?: "top"|"bottom" }
pub fn tool_show_status_overlay(args: &Value) -> NativeToolResult {
    let text = match args.get("text").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return NativeToolResult::text_only("Error: 'text' is required".to_string()),
    };
    let position = args
        .get("position")
        .and_then(|v| v.as_str())
        .unwrap_or("top");

    #[cfg(not(windows))]
    {
        return NativeToolResult::text_only(
            "Status overlay is currently only supported on Windows.".to_string(),
        );
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use std::process::{Command, Stdio};

        let mut lock = OVERLAY.lock().unwrap();

        // Kill existing overlay if any
        if let Some(mut old) = lock.take() {
            // Signal exit via control file, then force kill with bounded wait
            let _ = std::fs::write(&old.control_file, "__EXIT__");
            std::thread::sleep(std::time::Duration::from_millis(300));
            let _ = old.child.kill();
            // Bounded wait — never block more than 1 second
            for _ in 0..10 {
                if old.child.try_wait().ok().flatten().is_some() { break; }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            let _ = std::fs::remove_file(&old.control_file);
        }

        // Write initial text to control file
        let ctrl = control_file_path();
        if let Err(e) = std::fs::write(&ctrl, text) {
            return NativeToolResult::text_only(format!("Error writing control file: {e}"));
        }

        // Inject $ControlFile variable, then run the script body
        let script_body = &OVERLAY_PS_SCRIPT
            [OVERLAY_PS_SCRIPT.find("Add-Type").unwrap_or(0)..];
        let script_with_param = format!(
            "$ControlFile = '{}'; $Position = '{}'; {}",
            ctrl.to_string_lossy().replace('\'', "''"),
            position,
            script_body,
        );

        let mut cmd = Command::new("powershell");
        cmd.args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script_with_param,
        ]);
        cmd.stdin(Stdio::null());
        cmd.stdout(Stdio::null());
        cmd.stderr(Stdio::null());
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW

        match cmd.spawn() {
            Ok(child) => {
                *lock = Some(OverlayState {
                    child,
                    control_file: ctrl,
                });
                NativeToolResult::text_only(format!(
                    "Status overlay shown: \"{text}\""
                ))
            }
            Err(e) => {
                let _ = std::fs::remove_file(&ctrl);
                NativeToolResult::text_only(format!("Error spawning overlay: {e}"))
            }
        }
    }
}

/// Update the text on an existing status overlay.
/// Args: { text: String }
pub fn tool_update_status_overlay(args: &Value) -> NativeToolResult {
    let text = match args.get("text").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return NativeToolResult::text_only("Error: 'text' is required".to_string()),
    };

    let mut lock = OVERLAY.lock().unwrap();
    match lock.as_mut() {
        Some(state) => {
            // Check if process still alive
            match state.child.try_wait() {
                Ok(Some(_)) => {
                    let _ = std::fs::remove_file(&state.control_file);
                    *lock = None;
                    NativeToolResult::text_only(
                        "Error: overlay exited. Call show_status_overlay first.".to_string(),
                    )
                }
                Ok(None) => {
                    match std::fs::write(&state.control_file, text) {
                        Ok(()) => NativeToolResult::text_only(format!(
                            "Overlay updated: \"{text}\""
                        )),
                        Err(e) => NativeToolResult::text_only(format!(
                            "Error updating overlay: {e}"
                        )),
                    }
                }
                Err(e) => {
                    NativeToolResult::text_only(format!("Error checking overlay process: {e}"))
                }
            }
        }
        None => NativeToolResult::text_only(
            "Error: no overlay shown. Call show_status_overlay first.".to_string(),
        ),
    }
}

/// Hide (dismiss) the status overlay.
pub fn tool_hide_status_overlay(_args: &Value) -> NativeToolResult {
    let mut lock = OVERLAY.lock().unwrap();
    match lock.take() {
        Some(mut state) => {
            // Signal exit via control file, then force kill with bounded wait
            let _ = std::fs::write(&state.control_file, "__EXIT__");
            std::thread::sleep(std::time::Duration::from_millis(500));
            if state.child.try_wait().ok().flatten().is_none() {
                let _ = state.child.kill();
                for _ in 0..10 {
                    if state.child.try_wait().ok().flatten().is_some() { break; }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            }
            let _ = std::fs::remove_file(&state.control_file);
            NativeToolResult::text_only("Status overlay hidden.".to_string())
        }
        None => NativeToolResult::text_only("No overlay was shown.".to_string()),
    }
}
