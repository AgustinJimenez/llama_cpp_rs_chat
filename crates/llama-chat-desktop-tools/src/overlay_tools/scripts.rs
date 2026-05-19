//! Platform-specific overlay scripts.
//!
//! Each constant holds the full script text for a given platform:
//! - Windows: PowerShell WinForms
//! - macOS: AppleScript / ObjC bridge via osascript
//! - Linux: bash wrapper using zenity / yad / kdialog / notify-send

/// The PowerShell WinForms script. Takes $ControlFile variable, polls it for text
/// updates every 100ms. "__EXIT__" or file deletion closes the window.
/// Hidden from Alt+Tab (WS_EX_TOOLWINDOW). Has a clickable X button so the user
/// can dismiss it manually. Does not steal focus on show.
#[cfg(windows)]
pub const OVERLAY_PS_SCRIPT: &str = r#"
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
pub const OVERLAY_APPLESCRIPT: &str = r#"
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
pub const OVERLAY_BASH_SCRIPT: &str = r##"#!/bin/bash
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
