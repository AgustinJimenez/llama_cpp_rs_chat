//! Status overlay tools: persistent on-screen bar showing automation progress.
//!
//! - **Windows**: PowerShell WinForms window that polls a control file for text updates.
//! - **macOS**: `osascript` running AppleScript that displays a persistent floating panel polling a control file.
//! - **Linux**: `zenity --progress` fed updates via a control file polling wrapper, with `notify-send` fallback.
//!
//! Lifecycle: show_status_overlay (spawn) → update_status_overlay (write file) → hide_status_overlay (write __EXIT__).

use serde_json::Value;
use std::path::PathBuf;
use std::process::Child;
use std::sync::Mutex;

use super::NativeToolResult;

// ─── State persistence ───────────────────────────────────────────────────────

struct OverlayState {
    child: Child,
    control_file: PathBuf,
}

impl Drop for OverlayState {
    fn drop(&mut self) {
        let _ = std::fs::write(&self.control_file, "__EXIT__");
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.control_file);
    }
}

lazy_static::lazy_static! {
    /// Holds the overlay child process and control file path (platform-agnostic).
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

/// macOS AppleScript: creates a persistent floating panel via a tiny Cocoa app
/// assembled from `osascript`. Polls the control file every 0.1s for updates.
/// "__EXIT__" or file deletion closes the panel.
///
/// AppleScript unfortunately cannot create true always-on-top panels via pure
/// `display dialog`, so we use an ObjC bridge through osascript to build a real
/// NSPanel. This gives us the same behaviour as the Windows WinForms overlay:
/// no dock icon, always on top, not focusable, slim bar at top or bottom of screen.
#[cfg(target_os = "macos")]
const OVERLAY_APPLESCRIPT: &str = r#"
use framework "Cocoa"
use framework "AppKit"
use scripting additions

on run argv
    set controlFile to item 1 of argv
    set overlayPosition to item 2 of argv

    -- Read initial text
    try
        set initialText to do shell script "cat " & quoted form of controlFile
    on error
        set initialText to "..."
    end try

    -- Create the window using ObjC bridge
    set screenFrame to current application's NSScreen's mainScreen()'s frame()
    set screenWidth to item 1 of item 2 of screenFrame
    set screenHeight to item 2 of item 2 of screenFrame
    set barHeight to 32

    if overlayPosition is "bottom" then
        set barY to 0
    else
        set barY to screenHeight - barHeight
    end if

    set panelRect to current application's NSMakeRect(0, barY, screenWidth, barHeight)

    -- NSPanel: nonactivating, utility style, borderless
    -- style mask 0 = borderless
    set panelStyle to 0
    set thePanel to current application's NSPanel's alloc()'s initWithContentRect:panelRect styleMask:panelStyle backing:2 defer:false
    thePanel's setLevel:(current application's NSStatusWindowLevel)
    thePanel's setHidesOnDeactivate:false
    thePanel's setCanBecomeKeyWindow:false
    thePanel's setCollectionBehavior:((current application's NSWindowCollectionBehaviorCanJoinAllSpaces) + (current application's NSWindowCollectionBehaviorStationary))
    thePanel's setAlphaValue:0.88
    thePanel's setBackgroundColor:(current application's NSColor's colorWithRed:0.098 green:0.098 blue:0.118 alpha:1.0)

    -- Text field
    set textRect to current application's NSMakeRect(10, 0, screenWidth - 50, barHeight)
    set textField to current application's NSTextField's alloc()'s initWithFrame:textRect
    textField's setStringValue:initialText
    textField's setTextColor:(current application's NSColor's whiteColor())
    textField's setBackgroundColor:(current application's NSColor's clearColor())
    textField's setBordered:false
    textField's setEditable:false
    textField's setSelectable:false
    textField's setFont:(current application's NSFont's systemFontOfSize:13)
    thePanel's contentView()'s addSubview:textField

    -- Close button (X) as a clickable text field
    set closeBtnRect to current application's NSMakeRect(screenWidth - 40, 0, 36, barHeight)
    set closeBtn to current application's NSButton's alloc()'s initWithFrame:closeBtnRect
    closeBtn's setTitle:(character id 10005) -- Unicode X mark
    closeBtn's setBordered:false
    closeBtn's setBezelStyle:0
    (closeBtn's cell())'s setBackgroundColor:(current application's NSColor's clearColor())
    thePanel's contentView()'s addSubview:closeBtn

    thePanel's orderFrontRegardless()

    -- Poll control file in a loop
    repeat
        delay 0.1
        try
            set currentText to do shell script "cat " & quoted form of controlFile
        on error
            -- File deleted → exit
            thePanel's orderOut:(missing value)
            return
        end try
        if currentText is "__EXIT__" then
            thePanel's orderOut:(missing value)
            return
        end if
        textField's setStringValue:currentText
    end repeat
end run
"#;

/// Linux: A bash wrapper that launches `zenity --progress --pulsate` and polls
/// the control file for updates. Text changes are sent to zenity's stdin to update
/// the window title/text. "__EXIT__" or file deletion terminates the process.
///
/// Falls back to plain `notify-send` if zenity is not installed.
#[cfg(target_os = "linux")]
const OVERLAY_BASH_SCRIPT: &str = r##"#!/bin/bash
CONTROL_FILE="$1"
POSITION="$2"

if [ ! -f "$CONTROL_FILE" ]; then
    exit 1
fi

INITIAL_TEXT=$(cat "$CONTROL_FILE" 2>/dev/null || echo "...")

# Check for zenity
if command -v zenity &>/dev/null; then
    # zenity --progress: the window title shows our text, pulsate mode.
    # We pipe percentage updates and use --auto-close when we write 100.
    # But for a persistent bar, we just keep writing lines to stdin.
    (
        LAST_TEXT="$INITIAL_TEXT"
        echo "# $INITIAL_TEXT"
        echo "0"  # keep pulsating
        while true; do
            sleep 0.1
            if [ ! -f "$CONTROL_FILE" ]; then
                echo "100"  # triggers auto-close
                break
            fi
            CURRENT=$(cat "$CONTROL_FILE" 2>/dev/null)
            if [ "$CURRENT" = "__EXIT__" ]; then
                echo "100"
                break
            fi
            if [ "$CURRENT" != "$LAST_TEXT" ]; then
                echo "# $CURRENT"
                LAST_TEXT="$CURRENT"
            fi
        done
    ) | zenity --progress \
        --title="Claude Status" \
        --text="$INITIAL_TEXT" \
        --pulsate \
        --no-cancel \
        --auto-close \
        --width=500 \
        2>/dev/null

elif command -v yad &>/dev/null; then
    # yad is a zenity fork with more features — same interface
    (
        LAST_TEXT="$INITIAL_TEXT"
        echo "# $INITIAL_TEXT"
        echo "0"
        while true; do
            sleep 0.1
            if [ ! -f "$CONTROL_FILE" ]; then
                echo "100"
                break
            fi
            CURRENT=$(cat "$CONTROL_FILE" 2>/dev/null)
            if [ "$CURRENT" = "__EXIT__" ]; then
                echo "100"
                break
            fi
            if [ "$CURRENT" != "$LAST_TEXT" ]; then
                echo "# $CURRENT"
                LAST_TEXT="$CURRENT"
            fi
        done
    ) | yad --progress \
        --title="Claude Status" \
        --text="$INITIAL_TEXT" \
        --pulsate \
        --no-buttons \
        --auto-close \
        --width=500 \
        --undecorated \
        --on-top \
        --no-focus \
        2>/dev/null

elif command -v kdialog &>/dev/null; then
    # KDE: kdialog does not support persistent progress via stdin easily,
    # so we use a DBus-based approach
    DBUS_REF=$(kdialog --progressbar "$INITIAL_TEXT" 0)
    if [ -n "$DBUS_REF" ]; then
        qdbus $DBUS_REF setLabelText "$INITIAL_TEXT" 2>/dev/null
        LAST_TEXT="$INITIAL_TEXT"
        while true; do
            sleep 0.1
            if [ ! -f "$CONTROL_FILE" ]; then
                qdbus $DBUS_REF close 2>/dev/null
                break
            fi
            CURRENT=$(cat "$CONTROL_FILE" 2>/dev/null)
            if [ "$CURRENT" = "__EXIT__" ]; then
                qdbus $DBUS_REF close 2>/dev/null
                break
            fi
            if [ "$CURRENT" != "$LAST_TEXT" ]; then
                qdbus $DBUS_REF setLabelText "$CURRENT" 2>/dev/null
                LAST_TEXT="$CURRENT"
            fi
        done
    fi

else
    # Fallback: just use notify-send for the initial text, no persistent overlay
    if command -v notify-send &>/dev/null; then
        notify-send "Claude Status" "$INITIAL_TEXT"
        # Poll and send notifications on change
        LAST_TEXT="$INITIAL_TEXT"
        while true; do
            sleep 1
            if [ ! -f "$CONTROL_FILE" ]; then
                break
            fi
            CURRENT=$(cat "$CONTROL_FILE" 2>/dev/null)
            if [ "$CURRENT" = "__EXIT__" ]; then
                break
            fi
            if [ "$CURRENT" != "$LAST_TEXT" ]; then
                notify-send "Claude Status" "$CURRENT"
                LAST_TEXT="$CURRENT"
            fi
        done
    else
        echo "No supported dialog tool found (zenity, yad, kdialog, notify-send)" >&2
        exit 1
    fi
fi

# Clean up control file on exit
rm -f "$CONTROL_FILE" 2>/dev/null
"##;

// ─── Tool functions ──────────────────────────────────────────────────────────

fn control_file_path() -> PathBuf {
    std::env::temp_dir().join("claude_overlay_ctrl.txt")
}

/// Kill an existing overlay process with a bounded wait.
fn kill_existing_overlay(old: &mut OverlayState) {
    let _ = std::fs::write(&old.control_file, "__EXIT__");
    std::thread::sleep(std::time::Duration::from_millis(300));
    let _ = old.child.kill();
    // Bounded wait — never block more than 1 second
    for _ in 0..10 {
        if old.child.try_wait().ok().flatten().is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let _ = std::fs::remove_file(&old.control_file);
}

/// Show a persistent status overlay bar on screen.
/// Args: { text: String, position?: "top"|"bottom" }
pub fn tool_show_status_overlay(args: &Value) -> NativeToolResult {
    let text = match args.get("text").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return super::tool_error("show_status_overlay", "'text' is required"),
    };
    let position = args
        .get("position")
        .and_then(|v| v.as_str())
        .unwrap_or("top");

    // Unsupported platforms
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        let _ = (text, position);
        return NativeToolResult::text_only(
            "Status overlay is not supported on this platform.".to_string(),
        );
    }

    #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
    {
        use std::process::{Command, Stdio};

        let mut lock = OVERLAY.lock().unwrap_or_else(|poisoned| {
            crate::log_warn!("system", "Mutex poisoned in OVERLAY, recovering");
            poisoned.into_inner()
        });

        // Kill existing overlay if any
        if let Some(mut old) = lock.take() {
            kill_existing_overlay(&mut old);
        }

        // Write initial text to control file
        let ctrl = control_file_path();
        if let Err(e) = std::fs::write(&ctrl, text) {
            return super::tool_error("show_status_overlay", format!("writing control file: {e}"));
        }

        // ── Windows: PowerShell WinForms ──
        #[cfg(windows)]
        let spawn_result = {
            use std::os::windows::process::CommandExt;

            let script_body =
                &OVERLAY_PS_SCRIPT[OVERLAY_PS_SCRIPT.find("Add-Type").unwrap_or(0)..];
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
            cmd.spawn()
        };

        // ── macOS: osascript with AppleScript ──
        #[cfg(target_os = "macos")]
        let spawn_result = {
            let mut cmd = Command::new("osascript");
            cmd.arg("-");
            cmd.arg(ctrl.to_string_lossy().as_ref());
            cmd.arg(position);
            cmd.stdin(Stdio::piped());
            cmd.stdout(Stdio::null());
            cmd.stderr(Stdio::null());

            // We pipe the script via stdin so we don't need a temp file
            match cmd.spawn() {
                Ok(mut child) => {
                    // Write the AppleScript to osascript's stdin, then close it
                    if let Some(mut stdin) = child.stdin.take() {
                        use std::io::Write;
                        let _ = stdin.write_all(OVERLAY_APPLESCRIPT.as_bytes());
                        // stdin is dropped here, closing the pipe
                    }
                    Ok(child)
                }
                Err(e) => Err(e),
            }
        };

        // ── Linux: bash script with zenity/yad/kdialog/notify-send ──
        #[cfg(target_os = "linux")]
        let spawn_result = {
            // bash -c <script> _ <arg1> <arg2>  →  $1=arg1, $2=arg2
            let mut cmd = Command::new("bash");
            cmd.arg("-c");
            cmd.arg(OVERLAY_BASH_SCRIPT);
            cmd.arg("_"); // becomes $0 (unused)
            cmd.arg(ctrl.to_string_lossy().as_ref()); // $1 = control file
            cmd.arg(position); // $2 = position
            cmd.stdin(Stdio::null());
            cmd.stdout(Stdio::null());
            cmd.stderr(Stdio::null());
            cmd.spawn()
        };

        match spawn_result {
            Ok(child) => {
                *lock = Some(OverlayState {
                    child,
                    control_file: ctrl,
                });
                NativeToolResult::text_only(format!("Status overlay shown: \"{text}\""))
            }
            Err(e) => {
                let _ = std::fs::remove_file(&ctrl);
                super::tool_error("show_status_overlay", format!("spawning overlay: {e}"))
            }
        }
    }
}

/// Update the text on an existing status overlay.
/// Args: { text: String }
pub fn tool_update_status_overlay(args: &Value) -> NativeToolResult {
    let text = match args.get("text").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return super::tool_error("update_status_overlay", "'text' is required"),
    };

    let mut lock = OVERLAY.lock().unwrap_or_else(|poisoned| {
        crate::log_warn!("system", "Mutex poisoned in OVERLAY, recovering");
        poisoned.into_inner()
    });
    match lock.as_mut() {
        Some(state) => {
            // Check if process still alive
            match state.child.try_wait() {
                Ok(Some(_)) => {
                    let _ = std::fs::remove_file(&state.control_file);
                    *lock = None;
                    super::tool_error(
                        "update_status_overlay",
                        "overlay exited. Call show_status_overlay first.",
                    )
                }
                Ok(None) => {
                    match std::fs::write(&state.control_file, text) {
                        Ok(()) => NativeToolResult::text_only(format!(
                            "Overlay updated: \"{text}\""
                        )),
                        Err(e) => super::tool_error(
                            "update_status_overlay",
                            format!("updating overlay: {e}"),
                        ),
                    }
                }
                Err(e) => {
                    super::tool_error("update_status_overlay", format!("checking overlay process: {e}"))
                }
            }
        }
        None => super::tool_error(
            "update_status_overlay",
            "no overlay shown. Call show_status_overlay first.",
        ),
    }
}

/// Hide (dismiss) the status overlay.
pub fn tool_hide_status_overlay(_args: &Value) -> NativeToolResult {
    let mut lock = OVERLAY.lock().unwrap_or_else(|poisoned| {
        crate::log_warn!("system", "Mutex poisoned in OVERLAY, recovering");
        poisoned.into_inner()
    });
    match lock.take() {
        Some(mut state) => {
            // Signal exit via control file, then force kill with bounded wait
            let _ = std::fs::write(&state.control_file, "__EXIT__");
            std::thread::sleep(std::time::Duration::from_millis(500));
            if state.child.try_wait().ok().flatten().is_none() {
                let _ = state.child.kill();
                for _ in 0..10 {
                    if state.child.try_wait().ok().flatten().is_some() {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            }
            let _ = std::fs::remove_file(&state.control_file);
            NativeToolResult::text_only("Status overlay hidden.".to_string())
        }
        None => NativeToolResult::text_only("No overlay was shown.".to_string()),
    }
}
