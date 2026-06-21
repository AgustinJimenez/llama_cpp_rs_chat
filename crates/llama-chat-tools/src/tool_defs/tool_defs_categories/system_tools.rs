//! System, process, audio, and OS-level tool definitions.

use super::{p, Params, ToolDef};

pub static SYSTEM_TOOLS: &[ToolDef] = &[
    // ─── git_status ───
    ToolDef {
        name: "git_status",
        description: "Show the working tree status. Returns modified, staged, and untracked files.",
        params: Params::Simple(&[
            p("path", "string", "Repository path (defaults to current directory)"),
        ]),
        required: &[],
    },
    // ─── git_diff ───
    ToolDef {
        name: "git_diff",
        description: "Show git diff. By default shows unstaged changes. Set staged=true for staged changes.",
        params: Params::Simple(&[
            p("path", "string", "File path to diff, or omit for all changes"),
            p("staged", "boolean", "If true, show staged changes instead of unstaged (default: false)"),
        ]),
        required: &[],
    },
    // ─── git_commit ───
    ToolDef {
        name: "git_commit",
        description: "Commit changes with a message. By default commits staged changes only. Use all=true to auto-stage tracked modified files.",
        params: Params::Simple(&[
            p("message", "string", "Commit message"),
            p("all", "boolean", "If true, auto-stage tracked modified files before committing (git commit -a)"),
        ]),
        required: &["message"],
    },
    // ─── check_background_process ───
    ToolDef {
        name: "check_background_process",
        description: "Check on a background process launched with execute_command(background=true). Returns whether it is still running and any new output since last check. Use wait_seconds to pause before checking (combines wait + check in one call). Use max_checks to allow more polls for long-running servers (default 5).",
        params: Params::Simple(&[
            p("pid", "integer", "The PID returned by execute_command with background=true"),
            p("wait_seconds", "integer", "Seconds to wait before checking (1-30). Use this instead of calling wait separately."),
            p("max_checks", "integer", "Maximum number of times this process can be polled before the stop-polling warning fires (default 5). Set higher (e.g. 20) for long-running servers you need to monitor."),
        ]),
        required: &["pid"],
    },
    // ─── check_environment ───
    ToolDef {
        name: "check_environment",
        description: "Detect installed language runtimes and build tools in one call. Returns a table with tool name, version, and path for java, javac, mvn, gradle, node, npm, python, pip, rustc, cargo, go, git, docker, dotnet, php, ruby. Faster than probing each tool individually with execute_command.",
        params: Params::Simple(&[
            p("filter", "string", "Only show tools matching this substring (e.g. 'java', 'node'). Omit to show all."),
        ]),
        required: &[],
    },
    // ─── find_executable ───
    ToolDef {
        name: "find_executable",
        description: "Find an executable by name. Checks PATH first, then probes common installation directories (~/apache-mvn/..., ~/scoop/..., /usr/local/bin/, etc.). Returns the full path if found, or a list of locations searched. Use this instead of `where`/`which` when build tools (mvn, gradle, node, python, java) may not be on PATH.",
        params: Params::Simple(&[
            p("name", "string", "Executable name without extension (e.g. 'mvn', 'node', 'python', 'java')"),
        ]),
        required: &["name"],
    },
    // ─── list_background_processes ───
    ToolDef {
        name: "list_background_processes",
        description: "List all tracked background processes (running servers, daemons, etc.) with their PIDs, commands, and status. Also shows orphaned processes from previous sessions that are still running.",
        params: Params::Simple(&[]),
        required: &[],
    },
    // ─── list_processes ───
    ToolDef {
        name: "list_processes",
        description: "List running processes with PID and executable name. Optionally filter by name substring.",
        params: Params::Simple(&[
            p("filter", "string", "Filter by process name (case-insensitive substring)"),
        ]),
        required: &[],
    },
    // ─── kill_process ───
    ToolDef {
        name: "kill_process",
        description: "Terminate a process by name or PID. Refuses to kill system-critical processes (csrss, lsass, svchost, dwm, etc.).",
        params: Params::Simple(&[
            p("name", "string", "Process name to kill (kills all matching)"),
            p("pid", "integer", "Specific process ID to kill"),
            p("force", "boolean", "true (default): immediate kill. false: graceful WM_CLOSE then wait."),
            p("grace_ms", "integer", "Grace period in ms when force=false (default 5000, max 15000)"),
        ]),
        required: &[],
    },
    // ─── get_process_info ───
    ToolDef {
        name: "get_process_info",
        description: "Get resource info (memory usage, CPU time) for a process by PID or name.",
        params: Params::Simple(&[
            p("pid", "integer", "Process ID"),
            p("name", "string", "Process name (partial match)"),
        ]),
        required: &[],
    },
    // ─── get_process_tree ───
    ToolDef {
        name: "get_process_tree",
        description: "Show a process and all its child processes in a tree format.",
        params: Params::Simple(&[
            p("pid", "integer", "Root process ID"),
        ]),
        required: &["pid"],
    },
    // ─── wait_for_process_exit ───
    ToolDef {
        name: "wait_for_process_exit",
        description: "Block until a process exits or timeout. Useful for waiting on installers, builds, etc.",
        params: Params::Simple(&[
            p("pid", "integer", "Process ID to wait for"),
            p("name", "string", "Process name to wait for (alternative to pid)"),
            p("timeout_ms", "integer", "Maximum wait time (default 30000)"),
        ]),
        required: &[],
    },
    // ─── get_system_metrics ───
    ToolDef {
        name: "get_system_metrics",
        description: "Get system CPU usage, memory usage, and disk free space.",
        params: Params::Simple(&[]),
        required: &[],
    },
    // ─── get_pixel_color ───
    ToolDef {
        name: "get_pixel_color",
        description: "Get the color of a pixel at screen coordinates. Returns RGB values and hex code.",
        params: Params::Simple(&[
            p("x", "integer", "X coordinate (screen pixels)"),
            p("y", "integer", "Y coordinate (screen pixels)"),
        ]),
        required: &["x", "y"],
    },
    // ─── list_monitors ───
    ToolDef {
        name: "list_monitors",
        description: "List all connected monitors with name, resolution, position, scale factor, and primary status.",
        params: Params::Simple(&[
            p("index", "integer", "Get info for a specific monitor index only"),
        ]),
        required: &[],
    },
    // ─── get_system_volume ───
    ToolDef {
        name: "get_system_volume",
        description: "Get the current system audio volume (0-100) and muted state.",
        params: Params::Simple(&[]),
        required: &[],
    },
    // ─── set_system_volume ───
    ToolDef {
        name: "set_system_volume",
        description: "Set the system audio volume level.",
        params: Params::Simple(&[
            p("level", "integer", "Volume level 0-100"),
        ]),
        required: &["level"],
    },
    // ─── set_system_mute ───
    ToolDef {
        name: "set_system_mute",
        description: "Mute or unmute the system audio.",
        params: Params::Simple(&[
            p("muted", "boolean", "true to mute, false to unmute"),
        ]),
        required: &["muted"],
    },
    // ─── list_audio_devices ───
    ToolDef {
        name: "list_audio_devices",
        description: "List available audio output devices.",
        params: Params::Simple(&[]),
        required: &[],
    },
    // ─── read_registry ───
    ToolDef {
        name: "read_registry",
        description: "Read a value from the Windows registry. Supports REG_SZ (string) and REG_DWORD (integer) types.",
        params: Params::Simple(&[
            p("hive", "string", "Registry hive: HKCU or HKLM (default HKCU)"),
            p("key", "string", "Registry subkey path (e.g. 'SOFTWARE\\Microsoft\\Windows\\CurrentVersion')"),
            p("value", "string", "Value name to read (empty for default value)"),
        ]),
        required: &["key"],
    },
    // ─── click_tray_icon ───
    ToolDef {
        name: "click_tray_icon",
        description: "Find and click a system tray (notification area) icon by its tooltip text.",
        params: Params::Simple(&[
            p("name", "string", "Icon tooltip text to search for (partial match)"),
        ]),
        required: &["name"],
    },
    // ─── open_application ───
    ToolDef {
        name: "open_application",
        description: "Launch an application by name or path. Can open executables, URLs, files, or system apps (e.g. 'notepad', 'calc', 'https://google.com', 'C:\\\\path\\\\to\\\\app.exe').",
        params: Params::Simple(&[
            p("target", "string", "Application name, path, or URL to open"),
            p("args", "string", "Optional command-line arguments"),
        ]),
        required: &["target"],
    },
    // ─── execute_app_script ───
    ToolDef {
        name: "execute_app_script",
        description: "Execute a script inside a GPU-rendered application (Blender, etc.). These apps render with OpenGL/Vulkan so UI Automation tools don't work — use this instead. Supported apps: blender (Python/bpy).",
        params: Params::Simple(&[
            p("app", "string", "Application name: 'blender'"),
            p("code", "string", "Script source code (Python for Blender)"),
            p("file", "string", "Optional file to open (e.g. scene.blend)"),
            p("background", "boolean", "Run headless (default true). Set false to see GUI."),
        ]),
        required: &["app", "code"],
    },
    // ─── send_notification ───
    ToolDef {
        name: "send_notification",
        description: "Send a desktop notification (Windows toast / macOS notification / Linux notify-send). Use this to alert the user about progress, completion, or when you need their attention — especially during desktop automation so the user knows when to stop waiting.",
        params: Params::Simple(&[
            p("title", "string", "Notification title (default: 'Claude Code')"),
            p("message", "string", "Notification message body"),
        ]),
        required: &["message"],
    },
    // ─── show_status_overlay ───
    ToolDef {
        name: "show_status_overlay",
        description: "Show a persistent status bar overlay on screen. The bar is semi-transparent, always-on-top, click-through, and does not steal focus. Use this at the start of multi-step desktop automation to keep the user informed of progress. The overlay persists until you call hide_status_overlay.",
        params: Params::Simple(&[
            p("text", "string", "Text to display (e.g. '[Claude Code] Step 1/5: Opening Blender...')"),
            p("position", "string", "Bar position: 'top' (default) or 'bottom'"),
        ]),
        required: &["text"],
    },
    // ─── update_status_overlay ───
    ToolDef {
        name: "update_status_overlay",
        description: "Update the text on the existing status overlay bar. Must call show_status_overlay first. Near-instant — just a text update via IPC, no new process spawned.",
        params: Params::Simple(&[
            p("text", "string", "New text to display on the overlay"),
        ]),
        required: &["text"],
    },
    // ─── hide_status_overlay ───
    ToolDef {
        name: "hide_status_overlay",
        description: "Dismiss the status overlay bar. Call this when the automation sequence is complete.",
        params: Params::Simple(&[]),
        required: &[],
    },
    // ─── send_keys_to_window ───
    ToolDef {
        name: "send_keys_to_window",
        description: "Send keystrokes to a window. Prefer pid when you already know the target window identity. Default method 'post_message' works in background. Use method 'send_input' for foreground apps that don't respond to PostMessage (games, custom UIs).",
        params: Params::Simple(&[
            p("title", "string", "Window title to send keys to"),
            p("pid", "integer", "Specific process ID to target. Prefer this once you know the window identity."),
            p("keys", "string", "Key combo to send (e.g. 'ctrl+s', 'enter', 'alt+f4')"),
            p("text", "string", "Text characters to type"),
            p("method", "string", "Input method: post_message (default, background) or send_input (foreground, more reliable)"),
        ]),
        required: &[],
    },
    // ─── smart_wait ───
    ToolDef {
        name: "smart_wait",
        description: "Wait until screen changes, specific text appears via OCR, or both. Combines wait_for_screen_change + wait_for_text_on_screen.",
        params: Params::Simple(&[
            p("text", "string", "Text to wait for via OCR (optional if just waiting for screen change)"),
            p("timeout_ms", "integer", "Maximum wait time (default 10000, max 30000)"),
            p("threshold", "number", "Pixel change threshold percentage (default 1.0)"),
            p("mode", "string", "'any' (default) = return when either condition met, 'all' = wait for both"),
            p("monitor", "integer", "Monitor index (default 0)"),
            p("poll_ms", "integer", "Polling interval (default 500)"),
        ]),
        required: &[],
    },
    // ─── wait_for_notification ───
    ToolDef {
        name: "wait_for_notification",
        description: "Wait for a system notification matching a text filter (OCR-based detection).",
        params: Params::Simple(&[
            p("text_contains", "string", "Text to search for in the notification"),
            p("timeout_ms", "integer", "Maximum wait time (default 10000)"),
        ]),
        required: &["text_contains"],
    },
    // ─── dismiss_all_notifications ───
    ToolDef {
        name: "dismiss_all_notifications",
        description: "Clear/dismiss all system notifications from the notification center.",
        params: Params::Simple(&[]),
        required: &[],
    },
    // ─── sleep ───
    ToolDef {
        name: "sleep",
        description: "Wait for a specified number of seconds. Use between retries, or after starting a background server to give it time to initialize. Maximum 30 seconds.",
        params: Params::Simple(&[
            p("seconds", "integer", "Number of seconds to wait (1-30)"),
        ]),
        required: &["seconds"],
    },
    // ─── dialog_handler_stop ───
    ToolDef {
        name: "dialog_handler_stop",
        description: "Stop the background dialog handler and return the count of dialogs that were auto-handled.",
        params: Params::Simple(&[]),
        required: &[],
    },
];
