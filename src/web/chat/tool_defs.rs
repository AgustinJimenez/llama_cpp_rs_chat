//! Tool definitions in a compact format, expanded to JSON at runtime.
//!
//! Instead of ~2000 lines of `json!({...})` macros, each tool is defined as a
//! compact struct with name, description, parameters, and required fields.
//! Tools with complex parameter types (arrays, objects) use `RawParam` entries
//! that embed a pre-built `serde_json::Value`.

use serde_json::{json, Value};

/// A compact tool definition.
struct ToolDef {
    name: &'static str,
    description: &'static str,
    params: Params,
    required: &'static [&'static str],
}

/// Parameters — currently all tools use Simple; Mixed reserved for future use.
#[allow(dead_code)]
enum Params {
    /// All parameters are simple scalar types.
    Simple(&'static [ParamDef]),
    /// Some parameters need raw JSON (arrays, objects with nested schemas).
    Mixed(&'static [ParamDef], &'static [RawParam]),
}

/// A compact parameter definition for simple scalar types.
struct ParamDef {
    name: &'static str,
    param_type: &'static str, // "string", "integer", "boolean", "number"
    description: &'static str,
}

/// A parameter that needs a full JSON value (for arrays, objects, etc.).
#[allow(dead_code)]
struct RawParam {
    name: &'static str,
    build: fn() -> Value,
}

impl ToolDef {
    fn to_json(&self) -> Value {
        let mut properties = serde_json::Map::new();
        match &self.params {
            Params::Simple(defs) => {
                for p in *defs {
                    properties.insert(
                        p.name.to_string(),
                        json!({ "type": p.param_type, "description": p.description }),
                    );
                }
            }
            Params::Mixed(defs, raws) => {
                for p in *defs {
                    properties.insert(
                        p.name.to_string(),
                        json!({ "type": p.param_type, "description": p.description }),
                    );
                }
                for r in *raws {
                    properties.insert(r.name.to_string(), (r.build)());
                }
            }
        }
        let required: Vec<&str> = self.required.to_vec();
        json!({
            "name": self.name,
            "description": self.description,
            "parameters": {
                "type": "object",
                "properties": properties,
                "required": required,
            }
        })
    }
}

// ─── Verification helpers (cfg test) ────────────────────────────────────────
/// Number of tools defined. Used by tests to catch accidental omissions.
#[cfg(test)]
pub const EXPECTED_TOOL_COUNT: usize = 123;

// ─── Shorthand constructors ─────────────────────────────────────────────────
const fn p(name: &'static str, param_type: &'static str, description: &'static str) -> ParamDef {
    ParamDef { name, param_type, description }
}

// ─── The verification-related params (shared across many desktop tools) ─────
// Listed here so the tool entries stay compact.

static VERIFY_PARAMS: &[ParamDef] = &[
    p("verify_screen_change", "boolean", "If true, verify that the screen visibly changed after the action before returning."),
    p("verify_threshold_pct", "number", "Minimum percentage of sampled pixels that must change for verification to pass (default: 0.5)."),
    p("verify_timeout_ms", "integer", "Maximum time to wait for a visible change when verification is enabled (default: 1200)."),
    p("verify_poll_ms", "integer", "Polling interval for verification screenshots (default: 150)."),
    p("verify_x", "integer", "Optional absolute X for a custom verification region."),
    p("verify_y", "integer", "Optional absolute Y for a custom verification region."),
    p("verify_width", "integer", "Optional width for a custom verification region."),
    p("verify_height", "integer", "Optional height for a custom verification region."),
    p("verify_text", "string", "After action, OCR the verification region and confirm this text appears. Enables verify_screen_change automatically."),
];

/// Helper: merge multiple param slices into one Vec for tools that share verification params.
fn merge_params(base: &[ParamDef], extra: &[ParamDef]) -> Vec<(&'static str, Value)> {
    let mut out = Vec::new();
    for p in base.iter().chain(extra.iter()) {
        out.push((p.name, json!({ "type": p.param_type, "description": p.description })));
    }
    out
}

/// Build a tool JSON for tools that share the verification parameter set,
/// since we can't concatenate static slices at compile time.
fn tool_with_verify(
    name: &str,
    description: &str,
    base_params: &[ParamDef],
    extra_params: &[ParamDef],
    required: &[&str],
) -> Value {
    let mut properties = serde_json::Map::new();
    for (k, v) in merge_params(base_params, extra_params) {
        properties.insert(k.to_string(), v);
    }
    // Add verification params
    for vp in VERIFY_PARAMS {
        properties.insert(
            vp.name.to_string(),
            json!({ "type": vp.param_type, "description": vp.description }),
        );
    }
    let req: Vec<&str> = required.to_vec();
    json!({
        "name": name,
        "description": description,
        "parameters": {
            "type": "object",
            "properties": properties,
            "required": req,
        }
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tool definitions — 117 tools total
// ═══════════════════════════════════════════════════════════════════════════════

static ALL_TOOLS: &[ToolDef] = &[
    // ─── 1. read_file ───
    ToolDef {
        name: "read_file",
        description: "Read the contents of a file. Supports PDF, DOCX, XLSX, PPTX, EPUB, ODT, RTF, CSV, EML, ZIP, and non-UTF8 encoded files. Returns the file text (truncated at 100KB for large files). Binary files (exe, images, audio, etc.) are rejected — use a specialized tool for those.",
        params: Params::Simple(&[
            p("path", "string", "Path to the file to read"),
            p("offset", "integer", "Line number to start reading from (1-based). Use with limit to read specific portions of large files."),
            p("limit", "integer", "Maximum number of lines to read. Defaults to all lines."),
            p("pages", "string", "Page range for PDF files (e.g. '1-5', '3', '10-20'). Only for PDF files."),
        ]),
        required: &["path"],
    },
    // ─── 2. write_file ───
    ToolDef {
        name: "write_file",
        description: "Write content to a file. Creates parent directories if needed. Overwrites existing files.",
        params: Params::Simple(&[
            p("path", "string", "Path to write the file to"),
            p("content", "string", "The content to write to the file"),
        ]),
        required: &["path", "content"],
    },
    // ─── 3. edit_file ───
    ToolDef {
        name: "edit_file",
        description: "Replace exact text in a file. old_string must match exactly once in the file. Use this for small edits instead of rewriting the whole file with write_file.",
        params: Params::Simple(&[
            p("path", "string", "Path to the file to edit"),
            p("old_string", "string", "Exact text to find in the file (must appear exactly once)"),
            p("new_string", "string", "Text to replace it with"),
        ]),
        required: &["path", "old_string", "new_string"],
    },
    // ─── 4. undo_edit ───
    ToolDef {
        name: "undo_edit",
        description: "Revert the last edit_file operation on a file. Restores the file from its backup.",
        params: Params::Simple(&[
            p("path", "string", "Path to the file to restore"),
        ]),
        required: &["path"],
    },
    // ─── 5. insert_text ───
    ToolDef {
        name: "insert_text",
        description: "Insert text at a specific line number in a file. Line is 1-based. The text is inserted before the specified line.",
        params: Params::Simple(&[
            p("path", "string", "Path to the file"),
            p("line", "integer", "Line number to insert at (1-based)"),
            p("text", "string", "Text to insert"),
        ]),
        required: &["path", "line", "text"],
    },
    // ─── 6. search_files ───
    ToolDef {
        name: "search_files",
        description: "Search file contents across a directory by regex or literal pattern. Returns matching lines with file paths and line numbers. Use include to filter by file type (e.g. \"*.rs\").",
        params: Params::Simple(&[
            p("pattern", "string", "Regex or literal pattern to search for"),
            p("path", "string", "Directory to search in (default: current directory)"),
            p("include", "string", "Glob filter for file names (e.g. \"*.py\", \"*.rs\")"),
            p("context", "integer", "Number of context lines before/after each match (default: 0)"),
            p("exclude", "string", "Glob pattern to exclude (e.g. \"*_test.rs\", \"*.generated.*\")"),
        ]),
        required: &["pattern"],
    },
    // ─── 7. find_files ───
    ToolDef {
        name: "find_files",
        description: "Find files by name pattern recursively. Returns a list of matching file paths.",
        params: Params::Simple(&[
            p("pattern", "string", "File name pattern (e.g. \"*.tsx\", \"Cargo.*\", \"README*\")"),
            p("path", "string", "Directory to search in (default: current directory)"),
            p("exclude", "string", "Glob pattern to exclude (e.g. \"*.min.js\", \"*_test.*\")"),
        ]),
        required: &["pattern"],
    },
    // ─── 8. execute_python ───
    ToolDef {
        name: "execute_python",
        description: "Execute Python code. The code is written to a temp file and run with the Python interpreter. Supports multi-line code, imports, regex, and any valid Python. Returns stdout and stderr.",
        params: Params::Simple(&[
            p("code", "string", "The Python code to execute"),
        ]),
        required: &["code"],
    },
    // ─── 9. execute_command ───
    ToolDef {
        name: "execute_command",
        description: "Execute a shell command (git, npm, curl, etc.). You MUST set the background flag for every call.",
        params: Params::Simple(&[
            p("command", "string", "The shell command to execute"),
            p("background", "boolean", "REQUIRED. Set true for long-running processes (dev servers, watchers, daemons like 'php artisan serve', 'npm run dev', 'python -m http.server'). Set false for everything else (installs, builds, one-shot commands). If true, returns after 5s with initial output and the PID."),
            p("timeout", "integer", "Optional. Max seconds of inactivity (no output) before the command is killed. Default 120 (2 min). Resets every time the command produces output. Use higher values for commands with long silent phases."),
        ]),
        required: &["command", "background"],
    },
    // ─── 10. list_directory ───
    ToolDef {
        name: "list_directory",
        description: "List files and directories in a path. Shows name, size, and type for each entry.",
        params: Params::Simple(&[
            p("path", "string", "Directory path to list (defaults to current directory)"),
        ]),
        required: &[],
    },
    // web_search and web_fetch removed — browser tools (browser_navigate +
    // browser_get_text + browser_query) replace them with a real browser that
    // bypasses bot detection and handles JS-rendered pages.
    // The handlers still exist for backward compatibility with existing conversations.
    // ─── 13. open_url ───
    ToolDef {
        name: "open_url",
        description: "Open a URL in the user's external system browser outside the app. Only use this when the user explicitly asks to open something in their default browser or leave the in-app browser. Do NOT use this for web browsing, web search, reading pages, or screenshots inside the app — use browser_navigate and the other browser_* tools instead.",
        params: Params::Simple(&[
            p("url", "string", "The URL to open (must start with http:// or https://)"),
        ]),
        required: &["url"],
    },
    // ─── 14. git_status ───
    ToolDef {
        name: "git_status",
        description: "Show the working tree status. Returns modified, staged, and untracked files.",
        params: Params::Simple(&[
            p("path", "string", "Repository path (defaults to current directory)"),
        ]),
        required: &[],
    },
    // ─── 15. git_diff ───
    ToolDef {
        name: "git_diff",
        description: "Show git diff. By default shows unstaged changes. Set staged=true for staged changes.",
        params: Params::Simple(&[
            p("path", "string", "File path to diff, or omit for all changes"),
            p("staged", "boolean", "If true, show staged changes instead of unstaged (default: false)"),
        ]),
        required: &[],
    },
    // ─── 16. git_commit ───
    ToolDef {
        name: "git_commit",
        description: "Commit changes with a message. By default commits staged changes only. Use all=true to auto-stage tracked modified files.",
        params: Params::Simple(&[
            p("message", "string", "Commit message"),
            p("all", "boolean", "If true, auto-stage tracked modified files before committing (git commit -a)"),
        ]),
        required: &["message"],
    },
    // ─── 17. check_background_process ───
    ToolDef {
        name: "check_background_process",
        description: "Check on a background process launched with execute_command(background=true). Returns whether it is still running and any new output since last check. Use wait_seconds to pause before checking (combines wait + check in one call).",
        params: Params::Simple(&[
            p("pid", "integer", "The PID returned by execute_command with background=true"),
            p("wait_seconds", "integer", "Seconds to wait before checking (1-30). Use this instead of calling wait separately."),
        ]),
        required: &["pid"],
    },
    // ─── 18. lsp_query ───
    ToolDef {
        name: "lsp_query",
        description: "Query code intelligence: find definitions, references, symbols, diagnostics. Uses ctags (if available) with ripgrep fallback. For more precise results, you can install a real language server (rust-analyzer for Rust, typescript-language-server for TS, pyright for Python, gopls for Go, clangd for C/C++) and use execute_command to query it directly.",
        params: Params::Simple(&[
            p("action", "string", "Action: 'definition' (find where symbol is defined), 'references' (find all usages), 'symbols' (list symbols in file), 'hover' (get type info), 'diagnostics' (run language-specific type checker)"),
            p("symbol", "string", "Symbol name to query (e.g. 'MyStruct', 'my_function')"),
            p("file", "string", "File path for context (where the symbol is used)"),
            p("path", "string", "Project root directory to search in"),
        ]),
        required: &["action", "symbol"],
    },
    // ─── Camofox CAPTCHA interaction tools ───
    ToolDef {
        name: "camofox_click",
        description: "Click an element on the active Camofox browser tab (used after a CAPTCHA is detected during web_search). Provide the element ref shown in the page snapshot (e.g. 'e1', 'e3'). Returns a screenshot of the updated page.",
        params: Params::Simple(&[
            p("ref", "string", "Element reference to click (e.g. 'e1', 'e3')"),
        ]),
        required: &["ref"],
    },
    ToolDef {
        name: "camofox_screenshot",
        description: "Take a screenshot of the active Camofox browser tab. Use this to see the current state of a CAPTCHA page after interacting with it.",
        params: Params::Simple(&[]),
        required: &[],
    },
    ToolDef {
        name: "camofox_type",
        description: "Type text into an input field on the active Camofox browser tab. Used during CAPTCHA solving if text input is needed.",
        params: Params::Simple(&[
            p("ref", "string", "Element reference of the input field (e.g. 'e2')"),
            p("text", "string", "Text to type"),
            p("press_enter", "boolean", "Whether to press Enter after typing (default: false)"),
        ]),
        required: &["ref", "text"],
    },
    // ─── Browser view control (visible to user in chat UI) ───
    ToolDef {
        name: "open_browser_view",
        description: "Open the in-app browser view with a URL, visible to the user. Use this when you want to show the user a webpage directly in the chat interface. The user can see the page and interact with it. Useful for: showing search results, showing an article, or asking the user to solve a CAPTCHA. Creates a new Camofox tab and displays it live.",
        params: Params::Simple(&[
            p("url", "string", "Full URL to navigate to (e.g. 'https://example.com')"),
        ]),
        required: &["url"],
    },
    ToolDef {
        name: "close_browser_view",
        description: "Close the in-app browser view. Call this when the user is done viewing the page or the CAPTCHA is solved.",
        params: Params::Simple(&[]),
        required: &[],
    },
    // ─── Unified browser control tools (work for both web and Tauri) ───
    ToolDef {
        name: "browser_navigate",
        description: "Open or navigate the in-app browser to a URL. Creates a new session if none exists. Use this to start any browser-based task. The page becomes visible to the user in the browser view.",
        params: Params::Simple(&[
            p("url", "string", "URL to navigate to (with or without https://)"),
        ]),
        required: &["url"],
    },
    ToolDef {
        name: "browser_click",
        description: "Click an element in the browser using a CSS selector (e.g. 'button.submit', '#login', 'a[href*=\"signin\"]'). Returns immediately; effects appear in the next screenshot.",
        params: Params::Simple(&[
            p("selector", "string", "CSS selector of the element to click"),
        ]),
        required: &["selector"],
    },
    ToolDef {
        name: "browser_type",
        description: "Type text into an input field in the browser by CSS selector. Set press_enter=true to submit the form after typing.",
        params: Params::Simple(&[
            p("selector", "string", "CSS selector of the input field"),
            p("text", "string", "Text to type"),
            p("press_enter", "boolean", "Press Enter after typing (default: false)"),
        ]),
        required: &["selector", "text"],
    },
    ToolDef {
        name: "browser_query",
        description: "Extract structured data from the page using CSS selectors. Returns an array of matched elements with the requested attributes. Much simpler than browser_eval for data extraction.",
        params: Params::Simple(&[
            p("selector", "string", "CSS selector to match elements (e.g. '.titleline > a', 'article h2', 'table tr')"),
            p("attributes", "string", "Comma-separated attributes to extract: 'text' (innerText), 'href', 'src', 'class', 'id', 'html' (outerHTML), or any HTML attribute. Default: 'text'"),
            p("limit", "integer", "Max number of elements to return (default: 20)"),
        ]),
        required: &["selector"],
    },
    ToolDef {
        name: "browser_search",
        description: "Search the web using Google. Returns a list of results with titles, URLs, and snippets. Faster and more reliable than manually navigating to Google.",
        params: Params::Simple(&[
            p("query", "string", "The search query"),
            p("max_results", "integer", "Max results to return (default: 8)"),
        ]),
        required: &["query"],
    },
    ToolDef {
        name: "browser_eval",
        description: "Evaluate arbitrary JavaScript in the browser page context and return the result. Use for complex queries that browser_query can't handle (computed styles, DOM manipulation, event dispatch). Return value must be JSON-serializable.",
        params: Params::Simple(&[
            p("js", "string", "JavaScript expression or async function body. Last expression is returned."),
        ]),
        required: &["js"],
    },
    ToolDef {
        name: "browser_get_html",
        description: "Get the full HTML of the current page. Large output is summarized by default — pass a custom summary prompt to extract exactly what you need and save tokens.",
        params: Params::Simple(&[
            p("summary", "string", "'false' for raw HTML, 'true' (default) for generic summary, or a custom prompt to extract specific data (e.g. 'extract all article titles, URLs, and dates'). Custom prompts save tokens by returning only what you need."),
        ]),
        required: &[],
    },
    // browser_screenshot: not exposed — browser_get_text is better for content reading,
    // and the hidden webview can't capture visual screenshots. The handler still exists
    // in case models call it (returns page info + suggests alternatives).
    ToolDef {
        name: "browser_wait",
        description: "Wait for a CSS selector to appear in the page (after navigation, AJAX load, etc.). Returns true if found, false on timeout.",
        params: Params::Simple(&[
            p("selector", "string", "CSS selector to wait for"),
            p("timeout_ms", "integer", "Max wait time in milliseconds (default: 5000)"),
        ]),
        required: &["selector"],
    },
    ToolDef {
        name: "browser_close",
        description: "Close the active browser session and free its resources.",
        params: Params::Simple(&[]),
        required: &[],
    },
    ToolDef {
        name: "browser_get_text",
        description: "Get the visible text of the current page. Strips HTML tags. Large output is summarized by default — pass a custom summary prompt to extract exactly what you need and save tokens.",
        params: Params::Simple(&[
            p("summary", "string", "'false' for raw text, 'true' (default) for generic summary, or a custom prompt (e.g. 'summarize the main article in 3 sentences', 'extract the pricing table'). Custom prompts save tokens."),
        ]),
        required: &[],
    },
    ToolDef {
        name: "browser_get_links",
        description: "Get all links on the current page as {text, href} pairs. Large output is summarized by default — pass a custom prompt to filter (e.g. 'only article links, skip navigation').",
        params: Params::Simple(&[
            p("summary", "string", "'false' for all links raw, 'true' (default) for summary, or a custom prompt to filter/extract specific links."),
        ]),
        required: &[],
    },
    ToolDef {
        name: "browser_snapshot",
        description: "Get the accessibility snapshot — interactable elements (buttons, links, inputs) with labels. Compact view of what can be clicked/typed.",
        params: Params::Simple(&[
            p("summary", "string", "'false' for raw data, 'true' (default) for summary, or a custom prompt."),
        ]),
        required: &[],
    },
    ToolDef {
        name: "browser_scroll",
        description: "Scroll the page. Use 'amount' for relative scroll in pixels (positive=down, negative=up), or 'selector' to scroll an element into view.",
        params: Params::Simple(&[
            p("amount", "integer", "Pixels to scroll (positive=down). Optional if selector given."),
            p("selector", "string", "CSS selector of element to scroll into view. Optional if amount given."),
        ]),
        required: &[],
    },
    ToolDef {
        name: "browser_press_key",
        description: "Press a keyboard key in the active page. Examples: 'Enter', 'Tab', 'Escape', 'ArrowDown', 'PageDown', 'Control+a', 'Control+l'. Useful for forms and keyboard shortcuts.",
        params: Params::Simple(&[
            p("key", "string", "Key name (e.g. 'Enter', 'Tab', 'ArrowDown', 'Control+a')"),
        ]),
        required: &["key"],
    },
    // ─── 19. take_screenshot ───
    ToolDef {
        name: "take_screenshot",
        description: "Capture a screenshot of the user's screen. Returns the file path and image dimensions. Use monitor=-1 to list available monitors without capturing.",
        params: Params::Simple(&[
            p("monitor", "integer", "Monitor index (0=primary, 1,2..=other monitors). Use -1 to list available monitors."),
        ]),
        required: &[],
    },
    // ─── 19. move_mouse ───
    ToolDef {
        name: "move_mouse",
        description: "Move the mouse cursor to screen coordinates without clicking. Does not take a screenshot.",
        params: Params::Simple(&[
            p("x", "integer", "X coordinate in pixels from left edge of screen"),
            p("y", "integer", "Y coordinate in pixels from top edge of screen"),
        ]),
        required: &["x", "y"],
    },
    // ─── 20. list_windows ───
    ToolDef {
        name: "list_windows",
        description: "List all visible windows on the desktop with their titles, positions, sizes, process names, and state (minimized/maximized/focused). Use this to find windows before clicking or interacting with them. Returns an indexed list you can reference by number.",
        params: Params::Simple(&[
            p("filter", "string", "Optional case-insensitive filter. Only windows whose title or process name contains this string will be returned."),
            p("pid", "integer", "Filter to windows of this process ID"),
        ]),
        required: &[],
    },
    // ─── 21. get_cursor_position ───
    ToolDef {
        name: "get_cursor_position",
        description: "Get the current mouse cursor position on screen. Returns x,y coordinates in pixels.",
        params: Params::Simple(&[]),
        required: &[],
    },
    // ─── 22. focus_window ───
    ToolDef {
        name: "focus_window",
        description: "Bring a window to the foreground and give it focus. Prefer pid when you already know the target window identity; otherwise use a case-insensitive title or process-name filter. If the window is minimized, it will be restored first.",
        params: Params::Simple(&[
            p("title", "string", "Case-insensitive filter to match window title or process name (e.g. 'chrome', 'notepad')"),
            p("pid", "integer", "Specific process ID to target. Prefer this once you know the window identity."),
        ]),
        required: &[],
    },
    // ─── 23. minimize_window ───
    ToolDef {
        name: "minimize_window",
        description: "Minimize a window to the taskbar. Prefer pid when you already know the target window identity; otherwise use a case-insensitive title or process-name filter.",
        params: Params::Simple(&[
            p("title", "string", "Case-insensitive filter to match window title or process name"),
            p("pid", "integer", "Specific process ID to target. Prefer this once you know the window identity."),
        ]),
        required: &[],
    },
    // ─── 24. maximize_window ───
    ToolDef {
        name: "maximize_window",
        description: "Maximize a window to fill the screen. Prefer pid when you already know the target window identity; otherwise use a case-insensitive title or process-name filter.",
        params: Params::Simple(&[
            p("title", "string", "Case-insensitive filter to match window title or process name"),
            p("pid", "integer", "Specific process ID to target. Prefer this once you know the window identity."),
        ]),
        required: &[],
    },
    // ─── 25. close_window ───
    ToolDef {
        name: "close_window",
        description: "Close a window gracefully by sending WM_CLOSE. Prefer pid when you already know the target window identity; otherwise use a case-insensitive title or process-name filter. The application may show a save dialog before closing.",
        params: Params::Simple(&[
            p("title", "string", "Case-insensitive filter to match window title or process name"),
            p("pid", "integer", "Specific process ID to target. Prefer this once you know the window identity."),
        ]),
        required: &[],
    },
    // ─── 26. read_clipboard ───
    ToolDef {
        name: "read_clipboard",
        description: "Read the current text content from the system clipboard.",
        params: Params::Simple(&[]),
        required: &[],
    },
    // ─── 27. write_clipboard ───
    ToolDef {
        name: "write_clipboard",
        description: "Write text to the system clipboard, replacing its current content.",
        params: Params::Simple(&[
            p("text", "string", "The text to write to the clipboard"),
        ]),
        required: &["text"],
    },
    // ─── 28. resize_window ───
    ToolDef {
        name: "resize_window",
        description: "Move and/or resize a window by pid, title, or process name. Prefer pid when you already know the target window identity. Provide at least one of x, y, width, height.",
        params: Params::Simple(&[
            p("title", "string", "Window title or process name to match (case-insensitive substring)"),
            p("pid", "integer", "Specific process ID to target. Prefer this once you know the window identity."),
            p("x", "integer", "New X position (screen coordinates)"),
            p("y", "integer", "New Y position (screen coordinates)"),
            p("width", "integer", "New width in pixels"),
            p("height", "integer", "New height in pixels"),
        ]),
        required: &[],
    },
    // ─── 29. get_active_window ───
    ToolDef {
        name: "get_active_window",
        description: "Get info about the currently active (foreground) window: title, process, position, size.",
        params: Params::Simple(&[]),
        required: &[],
    },
    // ─── 30. wait_for_window ───
    ToolDef {
        name: "wait_for_window",
        description: "Wait for a window with matching pid, title, or process name to appear. Polls until found or timeout. Prefer pid when you already know the target window identity.",
        params: Params::Simple(&[
            p("title", "string", "Window title or process name to wait for"),
            p("pid", "integer", "Specific process ID to wait for. Prefer this once you know the window identity."),
            p("timeout_ms", "integer", "Maximum wait time in ms (default 10000, max 60000)"),
            p("poll_ms", "integer", "Polling interval in ms (default 200)"),
        ]),
        required: &[],
    },
    // ─── 31. get_pixel_color ───
    ToolDef {
        name: "get_pixel_color",
        description: "Get the color of a pixel at screen coordinates. Returns RGB values and hex code.",
        params: Params::Simple(&[
            p("x", "integer", "X coordinate (screen pixels)"),
            p("y", "integer", "Y coordinate (screen pixels)"),
        ]),
        required: &["x", "y"],
    },
    // ─── 32. list_monitors ───
    ToolDef {
        name: "list_monitors",
        description: "List all connected monitors with name, resolution, position, scale factor, and primary status.",
        params: Params::Simple(&[
            p("index", "integer", "Get info for a specific monitor index only"),
        ]),
        required: &[],
    },
    // ─── 33. screenshot_region ───
    ToolDef {
        name: "screenshot_region",
        description: "Capture a screenshot of a specific rectangular region of the screen. Returns the cropped image.",
        params: Params::Simple(&[
            p("x", "integer", "Left edge X coordinate"),
            p("y", "integer", "Top edge Y coordinate"),
            p("width", "integer", "Width of the region in pixels"),
            p("height", "integer", "Height of the region in pixels"),
            p("monitor", "integer", "Monitor index (default 0)"),
        ]),
        required: &["x", "y", "width", "height"],
    },
    // ─── 34. screenshot_diff ───
    ToolDef {
        name: "screenshot_diff",
        description: "Compare current screen to a baseline. First call with save_baseline=true to save, then call again to compare. Reports percentage of changed pixels and bounding box.",
        params: Params::Simple(&[
            p("save_baseline", "boolean", "If true, save current screen as baseline instead of comparing (default false)"),
            p("monitor", "integer", "Monitor index (default 0)"),
            p("highlight", "boolean", "Return image with red rectangle highlighting changed region"),
        ]),
        required: &[],
    },
    // ─── 35. ocr_screen ───
    ToolDef {
        name: "ocr_screen",
        description: "Extract text from the screen using OCR. Returns recognized text with line structure. Works on any app including GPU-rendered ones where get_ui_tree returns empty. Use 'engine' to select OCR backend: 'auto' (default, tries best available), 'ocrs' (Rust-native, fast), 'tesseract' (most accurate, requires install), 'native' (Windows WinRT / macOS Vision). Prefer window/pid to auto-crop instead of scanning full monitor.",
        params: Params::Simple(&[
            p("engine", "string", "OCR engine: 'auto' (default), 'ocrs' (Rust-native), 'tesseract' (CLI), 'native'/'winrt' (platform built-in)"),
            p("window", "string", "Window title to auto-crop OCR to (case-insensitive)"),
            p("title", "string", "Alias for window title/process filter to auto-crop OCR to"),
            p("pid", "integer", "Specific process ID to auto-crop OCR to. Prefer this once you know the window identity."),
            p("x", "integer", "Left edge of region to OCR"),
            p("y", "integer", "Top edge of region to OCR"),
            p("width", "integer", "Width of region"),
            p("height", "integer", "Height of region"),
            p("monitor", "integer", "Monitor index (default 0)"),
            p("language", "string", "OCR language code (tesseract/macOS Vision only)"),
        ]),
        required: &[],
    },
    // ─── 36. detect_ui_elements ───
    ToolDef {
        name: "detect_ui_elements",
        description: "Detect all interactive UI elements on screen using YOLO vision model + OCR. Returns a numbered list of elements with labels and coordinates. Works on ANY app including GPU-rendered ones (Unreal, Unity, Blender). Use this instead of get_ui_tree when the app uses custom rendering. Each element includes center coordinates for use with click_screen.",
        params: Params::Simple(&[
            p("monitor", "integer", "Monitor index (default 0)"),
            p("confidence", "number", "Detection confidence threshold 0.0-1.0 (default 0.15)"),
            p("ocr", "boolean", "Run OCR on detected elements to read labels (default true)"),
        ]),
        required: &[],
    },
    // ─── 37. ocr_find_text ───
    ToolDef {
        name: "ocr_find_text",
        description: "OCR the screen and find specific text, returning its bounding box coordinates. Prefer pid when you already know the target window identity; otherwise use window/title or a manual region to avoid scanning the full monitor.",
        params: Params::Simple(&[
            p("text", "string", "Text to search for (case-insensitive)"),
            p("window", "string", "Window title to auto-crop OCR search to (case-insensitive)"),
            p("title", "string", "Alias for window title/process filter to auto-crop OCR search to"),
            p("pid", "integer", "Specific process ID to auto-crop OCR search to. Prefer this once you know the window identity."),
            p("x", "integer", "Optional region X offset"),
            p("y", "integer", "Optional region Y offset"),
            p("width", "integer", "Optional region width"),
            p("height", "integer", "Optional region height"),
            p("monitor", "integer", "Monitor index (default 0)"),
            p("language", "string", "OCR language code (e.g. 'en-US', 'ja-JP'). macOS Vision only."),
        ]),
        required: &["text"],
    },
    // ─── 37. click_ui_element ───
    ToolDef {
        name: "click_ui_element",
        description: "Find a UI element by name and/or control type using UI Automation, then click its center. Works without screenshots — finds buttons, links, text fields by their accessible name.",
        params: Params::Simple(&[
            p("name", "string", "Element name to search for (case-insensitive substring match)"),
            p("control_type", "string", "Control type filter: Button, Edit, CheckBox, ComboBox, MenuItem, Hyperlink, etc."),
            p("title", "string", "Window title (default: active window)"),
            p("index", "integer", "Click the Nth match (0-based, default 0). Use with find_ui_elements to see all matches first."),
            p("delay_ms", "integer", "Delay before screenshot in ms (default 500)"),
        ]),
        required: &[],
    },
    // ─── 38. window_screenshot ───
    ToolDef {
        name: "window_screenshot",
        description: "Capture a screenshot of a specific window by title. Smaller and more focused than a full screen screenshot.",
        params: Params::Simple(&[
            p("title", "string", "Window title or app name to capture (case-insensitive substring match)"),
        ]),
        required: &["title"],
    },
    // ─── 39. open_application ───
    ToolDef {
        name: "open_application",
        description: "Launch an application by name or path. Can open executables, URLs, files, or system apps (e.g. 'notepad', 'calc', 'https://google.com', 'C:\\\\path\\\\to\\\\app.exe').",
        params: Params::Simple(&[
            p("target", "string", "Application name, path, or URL to open"),
            p("args", "string", "Optional command-line arguments"),
        ]),
        required: &["target"],
    },
    // ─── 40. wait_for_screen_change ───
    ToolDef {
        name: "wait_for_screen_change",
        description: "Wait until a screen region changes visually. Useful for waiting for loading indicators, animations, or content updates.",
        params: Params::Simple(&[
            p("x", "integer", "Region X (default 0)"),
            p("y", "integer", "Region Y (default 0)"),
            p("width", "integer", "Region width (default 200)"),
            p("height", "integer", "Region height (default 200)"),
            p("timeout_ms", "integer", "Max wait in ms (default 10000, max 30000)"),
            p("threshold", "number", "% of pixels that must change (default 5)"),
            p("monitor", "integer", "Monitor index (default 0)"),
        ]),
        required: &[],
    },
    // ─── 41. set_window_topmost ───
    ToolDef {
        name: "set_window_topmost",
        description: "Set a window to always-on-top or remove always-on-top. Prefer pid when you already know the target window identity. Useful for keeping reference windows visible while working.",
        params: Params::Simple(&[
            p("title", "string", "Window title to modify"),
            p("pid", "integer", "Specific process ID to target. Prefer this once you know the window identity."),
            p("topmost", "boolean", "true = always on top, false = remove (default true)"),
        ]),
        required: &[],
    },
    // ─── 42. invoke_ui_action ───
    ToolDef {
        name: "invoke_ui_action",
        description: "Invoke a UI Automation action on an element. Supports: invoke (click buttons), toggle (checkboxes), expand/collapse (tree nodes, dropdowns), select (list items), set_value (text fields). More reliable than coordinate clicking for standard Windows controls.",
        params: Params::Simple(&[
            p("name", "string", "Element name to match (case-insensitive substring)"),
            p("control_type", "string", "Control type filter (button, checkbox, edit, combobox, etc.)"),
            p("action", "string", "Action: invoke, toggle, expand, collapse, select, set_value"),
            p("value", "string", "Value for set_value action"),
            p("title", "string", "Window title (default: active window)"),
        ]),
        required: &["action"],
    },
    // ─── 43. read_ui_element_value ───
    ToolDef {
        name: "read_ui_element_value",
        description: "Read the current text value of a UI element (text field, label, status bar, etc.) using UI Automation ValuePattern.",
        params: Params::Simple(&[
            p("name", "string", "Element name to match (case-insensitive substring)"),
            p("control_type", "string", "Control type filter"),
            p("title", "string", "Window title (default: active window)"),
        ]),
        required: &[],
    },
    // ─── 44. wait_for_ui_element ───
    ToolDef {
        name: "wait_for_ui_element",
        description: "Wait until a UI element matching name/control_type appears in a window. Useful for waiting for dialogs, loading indicators, or UI state changes.",
        params: Params::Simple(&[
            p("name", "string", "Element name to wait for"),
            p("control_type", "string", "Control type to wait for"),
            p("title", "string", "Window title (default: active window)"),
            p("timeout_ms", "integer", "Max wait in ms (default 10000, max 30000)"),
            p("poll_ms", "integer", "Polling interval in ms (default 500, min 100)"),
        ]),
        required: &[],
    },
    // ─── 45. clipboard_image ───
    ToolDef {
        name: "clipboard_image",
        description: "Read or write images from/to the clipboard. Read returns the clipboard image as PNG. Write captures the screen and copies it to clipboard.",
        params: Params::Simple(&[
            p("action", "string", "read or write (default: read)"),
            p("monitor", "integer", "Monitor index for write action (default 0)"),
        ]),
        required: &[],
    },
    // ─── 46. find_ui_elements ───
    ToolDef {
        name: "find_ui_elements",
        description: "Search for ALL UI elements matching name/control_type in a window. Returns positions, sizes, and element descriptions. Useful for discovering available UI controls.",
        params: Params::Simple(&[
            p("name", "string", "Element name filter (case-insensitive substring)"),
            p("control_type", "string", "Control type filter (button, edit, checkbox, etc.)"),
            p("title", "string", "Window title (default: active window)"),
            p("max_results", "integer", "Max elements to return (default 10, max 50)"),
        ]),
        required: &[],
    },
    // ─── 47. execute_app_script ───
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
    // ─── 48. send_notification ───
    ToolDef {
        name: "send_notification",
        description: "Send a desktop notification (Windows toast / macOS notification / Linux notify-send). Use this to alert the user about progress, completion, or when you need their attention — especially during desktop automation so the user knows when to stop waiting.",
        params: Params::Simple(&[
            p("title", "string", "Notification title (default: 'Claude Code')"),
            p("message", "string", "Notification message body"),
        ]),
        required: &["message"],
    },
    // ─── 49. show_status_overlay ───
    ToolDef {
        name: "show_status_overlay",
        description: "Show a persistent status bar overlay on screen. The bar is semi-transparent, always-on-top, click-through, and does not steal focus. Use this at the start of multi-step desktop automation to keep the user informed of progress. The overlay persists until you call hide_status_overlay.",
        params: Params::Simple(&[
            p("text", "string", "Text to display (e.g. '[Claude Code] Step 1/5: Opening Blender...')"),
            p("position", "string", "Bar position: 'top' (default) or 'bottom'"),
        ]),
        required: &["text"],
    },
    // ─── 50. update_status_overlay ───
    ToolDef {
        name: "update_status_overlay",
        description: "Update the text on the existing status overlay bar. Must call show_status_overlay first. Near-instant — just a text update via IPC, no new process spawned.",
        params: Params::Simple(&[
            p("text", "string", "New text to display on the overlay"),
        ]),
        required: &["text"],
    },
    // ─── 51. hide_status_overlay ───
    ToolDef {
        name: "hide_status_overlay",
        description: "Dismiss the status overlay bar. Call this when the automation sequence is complete.",
        params: Params::Simple(&[]),
        required: &[],
    },
    // ─── 52. send_keys_to_window ───
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
    // ─── 53. snap_window ───
    ToolDef {
        name: "snap_window",
        description: "Snap a window to a screen position: left, right, top-left, top-right, bottom-left, bottom-right, center, maximize, restore. Prefer pid when you already know the target window identity. Uses monitor work area (excludes taskbar).",
        params: Params::Simple(&[
            p("title", "string", "Window title to snap"),
            p("pid", "integer", "Specific process ID to target. Prefer this once you know the window identity."),
            p("position", "string", "Position: left, right, top-left, top-right, bottom-left, bottom-right, center, maximize, restore"),
        ]),
        required: &["position"],
    },
    // ─── 54. list_processes ───
    ToolDef {
        name: "list_processes",
        description: "List running processes with PID and executable name. Optionally filter by name substring.",
        params: Params::Simple(&[
            p("filter", "string", "Filter by process name (case-insensitive substring)"),
        ]),
        required: &[],
    },
    // ─── 55. kill_process ───
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
    // ─── 56. find_and_click_text ───
    ToolDef {
        name: "find_and_click_text",
        description: "OCR the screen, find specific text, and click its center — all in one step. Combines ocr_find_text + click_screen. Use 'index' to click the Nth match.",
        params: Params::Simple(&[
            p("text", "string", "Text to find and click (case-insensitive)"),
            p("index", "integer", "Click the Nth match (0-based, default 0)"),
            p("monitor", "integer", "Monitor index (default 0)"),
            p("delay_ms", "integer", "Delay before screenshot in ms (default 500)"),
        ]),
        required: &["text"],
    },
    // ─── 57. type_into_element ───
    ToolDef {
        name: "type_into_element",
        description: "Find a UI element by name/type, click it to focus, then type text. Combines click_ui_element + type_text in one step.",
        params: Params::Simple(&[
            p("text", "string", "Text to type into the element"),
            p("name", "string", "Element name to find (case-insensitive substring)"),
            p("control_type", "string", "Control type filter (Edit, ComboBox, etc.)"),
            p("title", "string", "Window title (default: active window)"),
        ]),
        required: &["text"],
    },
    // ─── 58. get_window_text ───
    ToolDef {
        name: "get_window_text",
        description: "Extract all text content from a window via UI Automation tree walk. Returns text from labels, edit fields, and documents. Useful for reading window content without OCR.",
        params: Params::Simple(&[
            p("title", "string", "Window title (default: active window)"),
            p("max_chars", "integer", "Max characters to return (default 50000)"),
        ]),
        required: &[],
    },
    // ─── 59. file_dialog_navigate ───
    ToolDef {
        name: "file_dialog_navigate",
        description: "Navigate a file Open/Save dialog: sets the filename field and clicks the button. Useful for automating file selection in native dialogs.",
        params: Params::Simple(&[
            p("filename", "string", "File path or name to enter"),
            p("button", "string", "Button to click: Open, Save, etc. (default: Open)"),
            p("title", "string", "Dialog window title (auto-detected if omitted)"),
        ]),
        required: &["filename"],
    },
    // ─── 60. drag_and_drop_element ───
    ToolDef {
        name: "drag_and_drop_element",
        description: "Find two UI elements by name/type and drag from one to the other. Combines find_ui_element + mouse_drag.",
        params: Params::Simple(&[
            p("from_name", "string", "Source element name"),
            p("from_type", "string", "Source control type"),
            p("to_name", "string", "Target element name"),
            p("to_type", "string", "Target control type"),
            p("title", "string", "Window title (default: active window)"),
            p("delay_ms", "integer", "Delay before screenshot in ms (default 500)"),
        ]),
        required: &[],
    },
    // ─── 61. wait_for_text_on_screen ───
    ToolDef {
        name: "wait_for_text_on_screen",
        description: "Poll OCR until specified text appears on screen. Useful for waiting for loading to complete, dialogs to appear, or status text changes.",
        params: Params::Simple(&[
            p("text", "string", "Text to wait for (case-insensitive)"),
            p("timeout_ms", "integer", "Max wait in ms (default 10000, max 30000)"),
            p("poll_ms", "integer", "Polling interval in ms (default 1000, min 500)"),
            p("monitor", "integer", "Monitor index (default 0)"),
        ]),
        required: &["text"],
    },
    // ─── 62. get_context_menu ───
    ToolDef {
        name: "get_context_menu",
        description: "Right-click at coordinates to open a context menu, read menu items via UI Automation, and optionally click one. Returns a numbered list of menu items.",
        params: Params::Simple(&[
            p("x", "integer", "X coordinate to right-click"),
            p("y", "integer", "Y coordinate to right-click"),
            p("click_item", "string", "Menu item name to click (optional — just reads if omitted)"),
            p("delay_ms", "integer", "Delay before screenshot in ms (default 500)"),
        ]),
        required: &["x", "y"],
    },
    // ─── 63. scroll_element ───
    ToolDef {
        name: "scroll_element",
        description: "Find a UI element by name/type and scroll it. Uses mouse wheel at the element's center. Useful for scrolling specific panels or lists.",
        params: Params::Simple(&[
            p("name", "string", "Element name to find"),
            p("control_type", "string", "Control type filter"),
            p("direction", "string", "Scroll direction: up or down (default: down)"),
            p("amount", "integer", "Number of scroll clicks (default 3)"),
            p("title", "string", "Window title (default: active window)"),
        ]),
        required: &[],
    },
    // ─── 64. mouse_button ───
    ToolDef {
        name: "mouse_button",
        description: "Press or release a mouse button independently without clicking. Useful for hold-and-drag scenarios where you need separate press and release control.",
        params: Params::Simple(&[
            p("action", "string", "Action to perform: press or release"),
            p("button", "string", "Mouse button: left, right, middle (default: left)"),
            p("screenshot", "boolean", "Take screenshot after action (default true)"),
        ]),
        required: &["action"],
    },
    // ─── 65. switch_virtual_desktop ───
    ToolDef {
        name: "switch_virtual_desktop",
        description: "Switch to an adjacent virtual desktop using Ctrl+Win+Arrow keyboard shortcut.",
        params: Params::Simple(&[
            p("direction", "string", "Direction: left/prev or right/next"),
        ]),
        required: &["direction"],
    },
    // ─── 66. find_image_on_screen ───
    ToolDef {
        name: "find_image_on_screen",
        description: "Find a template image on the screen using pixel matching (SSD). Returns the position and confidence if found. Useful for finding icons, buttons, or UI elements by their visual appearance.",
        params: Params::Simple(&[
            p("template", "string", "Path to the template image file (PNG, JPEG, etc.)"),
            p("monitor", "integer", "Monitor index (default 0)"),
            p("confidence", "number", "Minimum confidence threshold 0.0-1.0 (default 0.9)"),
            p("step", "integer", "Search step size in pixels — larger = faster but less precise (default 2)"),
        ]),
        required: &["template"],
    },
    // ─── 67. get_process_info ───
    ToolDef {
        name: "get_process_info",
        description: "Get resource info (memory usage, CPU time) for a process by PID or name.",
        params: Params::Simple(&[
            p("pid", "integer", "Process ID"),
            p("name", "string", "Process name (partial match)"),
        ]),
        required: &[],
    },
    // ─── 68. paste ───
    ToolDef {
        name: "paste",
        description: "Paste clipboard contents at the current cursor position (Ctrl+V). Takes a screenshot after pasting.",
        params: Params::Simple(&[
            p("delay_ms", "integer", "Wait after paste before screenshot (default 300)"),
        ]),
        required: &[],
    },
    // ─── 69. clear_field ───
    ToolDef {
        name: "clear_field",
        description: "Clear the currently focused input field (Ctrl+A → Delete). Optionally type new text after clearing.",
        params: Params::Simple(&[
            p("then_type", "string", "Text to type after clearing the field"),
            p("delay_ms", "integer", "Wait after action (default 200)"),
        ]),
        required: &[],
    },
    // ─── 70. hover_element ───
    ToolDef {
        name: "hover_element",
        description: "Hover over a UI element by name/type to trigger tooltip or hover effects. Returns tooltip text if found.",
        params: Params::Simple(&[
            p("name", "string", "UI element name (partial match)"),
            p("control_type", "string", "UI control type (Button, Edit, etc.)"),
            p("title", "string", "Window title filter (default: active window)"),
            p("hover_ms", "integer", "How long to hover before capturing (default 800)"),
        ]),
        required: &[],
    },
    // ─── 71. handle_dialog ───
    ToolDef {
        name: "handle_dialog",
        description: "Detect and interact with modal dialogs. Lists dialog text and buttons, optionally clicks a button.",
        params: Params::Simple(&[
            p("button", "string", "Button name to click (e.g. 'OK', 'Cancel', 'Yes', 'Save')"),
        ]),
        required: &[],
    },
    // ─── 72. wait_for_element_state ───
    ToolDef {
        name: "wait_for_element_state",
        description: "Wait until a UI element reaches a specific state (exists, gone, visible, hidden).",
        params: Params::Simple(&[
            p("name", "string", "UI element name (partial match)"),
            p("control_type", "string", "UI control type filter"),
            p("state", "string", "Target state: exists, gone, visible, hidden"),
            p("title", "string", "Window title filter"),
            p("timeout_ms", "integer", "Maximum wait time (default 5000)"),
        ]),
        required: &["state"],
    },
    // ─── 73. move_to_monitor ───
    ToolDef {
        name: "move_to_monitor",
        description: "Move a window to a specific monitor by index. Preserves window size.",
        params: Params::Simple(&[
            p("title", "string", "Window title or process name filter"),
            p("monitor", "integer", "Target monitor index (default 0)"),
        ]),
        required: &["title"],
    },
    // ─── 74. set_window_opacity ───
    ToolDef {
        name: "set_window_opacity",
        description: "Set window transparency. 0 = fully transparent, 100 = fully opaque.",
        params: Params::Simple(&[
            p("title", "string", "Window title or process name filter"),
            p("opacity", "integer", "Opacity percentage 0-100 (default 100)"),
        ]),
        required: &["title"],
    },
    // ─── 75. highlight_point ───
    ToolDef {
        name: "highlight_point",
        description: "Draw a crosshair marker on a screenshot at specified coordinates. Useful for debugging coordinate targeting.",
        params: Params::Simple(&[
            p("x", "integer", "X coordinate"),
            p("y", "integer", "Y coordinate"),
            p("color", "string", "Marker color: red, green, blue, yellow (default red)"),
            p("size", "integer", "Marker size in pixels (default 20)"),
            p("monitor", "integer", "Monitor index (default 0)"),
        ]),
        required: &["x", "y"],
    },
    // ─── 76. ocr_region ───
    ToolDef {
        name: "ocr_region",
        description: "Perform OCR on a specific rectangular region of the screen. Returns recognized text and the cropped region image.",
        params: Params::Simple(&[
            p("x", "integer", "Left edge X coordinate"),
            p("y", "integer", "Top edge Y coordinate"),
            p("width", "integer", "Region width in pixels"),
            p("height", "integer", "Region height in pixels"),
            p("monitor", "integer", "Monitor index (default 0)"),
        ]),
        required: &["width", "height"],
    },
    // ─── 77. find_color_on_screen ───
    ToolDef {
        name: "find_color_on_screen",
        description: "Find pixels on screen matching a specific color (hex #RRGGBB) within tolerance. Returns coordinates of matches.",
        params: Params::Simple(&[
            p("color", "string", "Target color in hex format #RRGGBB"),
            p("tolerance", "integer", "Color matching tolerance per channel 0-255 (default 30)"),
            p("max_results", "integer", "Maximum matches to return (default 10)"),
            p("step", "integer", "Pixel scan step size (default 4, use 1 for thorough)"),
            p("region_x", "integer", "Optional region left X"),
            p("region_y", "integer", "Optional region top Y"),
            p("region_w", "integer", "Optional region width"),
            p("region_h", "integer", "Optional region height"),
            p("monitor", "integer", "Monitor index (default 0)"),
        ]),
        required: &["color"],
    },
    // ─── 78. read_registry ───
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
    // ─── 79. click_tray_icon ───
    ToolDef {
        name: "click_tray_icon",
        description: "Find and click a system tray (notification area) icon by its tooltip text.",
        params: Params::Simple(&[
            p("name", "string", "Icon tooltip text to search for (partial match)"),
        ]),
        required: &["name"],
    },
    // ─── 80. watch_window ───
    ToolDef {
        name: "watch_window",
        description: "Monitor for window changes (new windows, closed windows, title changes). Returns on first change or timeout.",
        params: Params::Simple(&[
            p("timeout_ms", "integer", "Maximum wait time (default 10000)"),
            p("filter", "string", "Only report changes for windows matching this filter"),
            p("poll_ms", "integer", "Polling interval (default 500)"),
        ]),
        required: &[],
    },
    // ─── 81. get_system_volume ───
    ToolDef {
        name: "get_system_volume",
        description: "Get the current system audio volume (0-100) and muted state.",
        params: Params::Simple(&[]),
        required: &[],
    },
    // ─── 82. set_system_volume ───
    ToolDef {
        name: "set_system_volume",
        description: "Set the system audio volume level.",
        params: Params::Simple(&[
            p("level", "integer", "Volume level 0-100"),
        ]),
        required: &["level"],
    },
    // ─── 83. set_system_mute ───
    ToolDef {
        name: "set_system_mute",
        description: "Mute or unmute the system audio.",
        params: Params::Simple(&[
            p("muted", "boolean", "true to mute, false to unmute"),
        ]),
        required: &["muted"],
    },
    // ─── 84. list_audio_devices ───
    ToolDef {
        name: "list_audio_devices",
        description: "List available audio output devices.",
        params: Params::Simple(&[]),
        required: &[],
    },
    // ─── 85. clear_clipboard ───
    ToolDef {
        name: "clear_clipboard",
        description: "Clear all content from the system clipboard.",
        params: Params::Simple(&[]),
        required: &[],
    },
    // ─── 86. save_window_layout ───
    ToolDef {
        name: "save_window_layout",
        description: "Save positions and sizes of all open windows to a named layout file.",
        params: Params::Simple(&[
            p("name", "string", "Layout name (used as filename)"),
        ]),
        required: &["name"],
    },
    // ─── 87. restore_window_layout ───
    ToolDef {
        name: "restore_window_layout",
        description: "Restore windows to positions saved in a named layout file.",
        params: Params::Simple(&[
            p("name", "string", "Layout name to restore"),
        ]),
        required: &["name"],
    },
    // ─── 88. wait_for_process_exit ───
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
    // ─── 89. get_process_tree ───
    ToolDef {
        name: "get_process_tree",
        description: "Show a process and all its child processes in a tree format.",
        params: Params::Simple(&[
            p("pid", "integer", "Root process ID"),
        ]),
        required: &["pid"],
    },
    // ─── 90. get_system_metrics ───
    ToolDef {
        name: "get_system_metrics",
        description: "Get system CPU usage, memory usage, and disk free space.",
        params: Params::Simple(&[]),
        required: &[],
    },
    // ─── 91. wait_for_notification ───
    ToolDef {
        name: "wait_for_notification",
        description: "Wait for a system notification matching a text filter (OCR-based detection).",
        params: Params::Simple(&[
            p("text_contains", "string", "Text to search for in the notification"),
            p("timeout_ms", "integer", "Maximum wait time (default 10000)"),
        ]),
        required: &["text_contains"],
    },
    // ─── 92. dismiss_all_notifications ───
    ToolDef {
        name: "dismiss_all_notifications",
        description: "Clear/dismiss all system notifications from the notification center.",
        params: Params::Simple(&[]),
        required: &[],
    },
    // ─── 93. start_screen_recording ───
    ToolDef {
        name: "start_screen_recording",
        description: "Start recording the screen to a video file using ffmpeg. Call stop_screen_recording to finish.",
        params: Params::Simple(&[
            p("output_path", "string", "Output file path (e.g. 'recording.mp4')"),
            p("fps", "integer", "Frames per second (default 15)"),
            p("monitor", "integer", "Monitor index (default 0)"),
        ]),
        required: &["output_path"],
    },
    // ─── 94. stop_screen_recording ───
    ToolDef {
        name: "stop_screen_recording",
        description: "Stop an active screen recording started by start_screen_recording.",
        params: Params::Simple(&[]),
        required: &[],
    },
    // ─── 95. capture_gif ───
    ToolDef {
        name: "capture_gif",
        description: "Capture a short animated GIF of the screen (pure Rust, no ffmpeg needed).",
        params: Params::Simple(&[
            p("output_path", "string", "Output GIF file path"),
            p("duration_ms", "integer", "Recording duration in ms (default 3000)"),
            p("fps", "integer", "Frames per second (default 10)"),
            p("monitor", "integer", "Monitor index (default 0)"),
        ]),
        required: &["output_path"],
    },
    // ─── 96. dialog_handler_stop ───
    ToolDef {
        name: "dialog_handler_stop",
        description: "Stop the background dialog handler and return the count of dialogs that were auto-handled.",
        params: Params::Simple(&[]),
        required: &[],
    },
    // ─── 97. smart_wait ───
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
    // ─── 98. click_and_verify ───
    ToolDef {
        name: "click_and_verify",
        description: "Find text on screen via OCR, click it, then verify that different expected text appeared. Combines find_and_click_text + OCR verification.",
        params: Params::Simple(&[
            p("click_text", "string", "Text to find and click"),
            p("expect_text", "string", "Text expected to appear after clicking"),
            p("timeout_ms", "integer", "Maximum wait for verification (default 5000)"),
            p("monitor", "integer", "Monitor index (default 0)"),
        ]),
        required: &["click_text", "expect_text"],
    },
    // ─── 99. list_mcp_servers ───
    ToolDef {
        name: "list_mcp_servers",
        description: "List all configured MCP (Model Context Protocol) servers with their connection status and available tools.",
        params: Params::Simple(&[]),
        required: &[],
    },
    // ─── 100. remove_mcp_server ───
    ToolDef {
        name: "remove_mcp_server",
        description: "Remove an MCP server by name. This disconnects the server and removes its configuration.",
        params: Params::Simple(&[
            p("name", "string", "Name of the MCP server to remove"),
        ]),
        required: &["name"],
    },
    // ─── 101. list_background_processes ───
    ToolDef {
        name: "list_background_processes",
        description: "List all tracked background processes (running servers, daemons, etc.) with their PIDs, commands, and status. Also shows orphaned processes from previous sessions that are still running.",
        params: Params::Simple(&[]),
        required: &[],
    },
    // ─── 102. sleep ───
    ToolDef {
        name: "sleep",
        description: "Wait for a specified number of seconds. Use between retries, or after starting a background server to give it time to initialize. Maximum 30 seconds.",
        params: Params::Simple(&[
            p("seconds", "integer", "Number of seconds to wait (1-30)"),
        ]),
        required: &["seconds"],
    },
    // ─── 103. send_telegram ───
    ToolDef {
        name: "send_telegram",
        description: "Send a notification message to the user via Telegram. Use to notify about task completion, errors, or important updates.",
        params: Params::Simple(&[
            p("message", "string", "The message text to send (supports Markdown formatting)"),
        ]),
        required: &["message"],
    },
    // ─── 104. spawn_agent ───
    ToolDef {
        name: "spawn_agent",
        description: "Spawn a sub-agent to handle an isolated sub-task. The agent gets a fresh context and returns a summary of what it did. Use for installation tasks, research, or any step that might use lots of context.",
        params: Params::Simple(&[
            p("task", "string", "The sub-task description for the agent to complete"),
            p("context", "string", "Additional context to provide to the agent (file contents, error messages, etc.)"),
        ]),
        required: &["task"],
    },
    // ─── 105. todo_write ───
    ToolDef {
        name: "todo_write",
        description: "Update the task checklist for this session. Use to track progress on multi-step tasks. Each todo has a status: pending, in_progress, or completed.",
        params: Params::Simple(&[
            p("todos", "string", "JSON array of todos: [{\"id\": 1, \"task\": \"description\", \"status\": \"pending|in_progress|completed\"}]"),
        ]),
        required: &["todos"],
    },
    // ─── 106. todo_read ───
    ToolDef {
        name: "todo_read",
        description: "Read the current task checklist for this session.",
        params: Params::Simple(&[]),
        required: &[],
    },
    // ─── 107. list_skills ───
    ToolDef {
        name: "list_skills",
        description: "List available prompt skills (reusable templates). Skills are .md files in the skills/ directory.",
        params: Params::Simple(&[]),
        required: &[],
    },
    // ─── 108. use_skill ───
    ToolDef {
        name: "use_skill",
        description: "Execute a skill (prompt template) by name. The skill's content becomes your instructions.",
        params: Params::Simple(&[
            p("name", "string", "Skill name to execute"),
            p("args", "string", "Arguments to substitute in the template (JSON object, e.g. {\"language\": \"python\", \"path\": \"./myapp\"})"),
        ]),
        required: &["name"],
    },
    // ─── 109. set_response_style ───
    ToolDef {
        name: "set_response_style",
        description: "Switch between brief and detailed response styles. Use 'brief' for short, action-focused responses (less explanation). Use 'detailed' for thorough explanations.",
        params: Params::Simple(&[
            p("style", "string", "Response style: 'brief' or 'detailed'"),
        ]),
        required: &["style"],
    },
];

// ═══════════════════════════════════════════════════════════════════════════════
// Tools with complex parameters that need runtime JSON construction.
// These are built by `all_tool_definitions_with_complex()` and merged in.
// ═══════════════════════════════════════════════════════════════════════════════

/// Tools that have verification params or complex (array/object) parameters.
/// Built at runtime because Rust const/static doesn't allow runtime JSON.
fn complex_tool_definitions() -> Vec<Value> {
    vec![
        // ─── click_screen (tool 19) — has verification params ───
        tool_with_verify(
            "click_screen",
            "Click the mouse at screen coordinates. Takes a screenshot after clicking by default; pass screenshot=false during long automation sessions to avoid bloating context. Use take_screenshot first to see the screen and identify coordinates. Use stealth=true for non-disruptive clicks that won't interrupt the user.",
            &[
                p("x", "integer", "X coordinate in pixels from left edge of screen"),
                p("y", "integer", "Y coordinate in pixels from top edge of screen"),
                p("button", "string", "Mouse button: 'left' (default), 'right', 'middle', 'double' (double left click)"),
                p("stealth", "boolean", "Stealth mode (Windows only, default: true): saves cursor position, clicks target in <0.1ms, restores cursor. User is not interrupted. Skips if user is mid-drag. Set to false for actions that need the cursor to stay at the target (e.g. hover menus)."),
                p("screenshot", "boolean", "Take a screenshot after clicking (default: true). Set to false to avoid embedding a full-screen capture in the result."),
                p("delay_ms", "integer", "Milliseconds to wait after clicking before taking screenshot (default: 500). Increase for slow UI animations."),
                p("dpi_aware", "boolean", "If true, coordinates are logical (96 DPI basis) and will be scaled to physical pixels by the system DPI factor (default: false)"),
            ],
            &[
                p("snap_to_screen", "boolean", "Clamp off-screen coordinates to nearest monitor edge"),
                p("timeout_ms", "integer", "Operation timeout in ms (1000-60000, default 20000)"),
            ],
            &["x", "y"],
        ),
        // ─── type_text (tool 20) — has verification params ───
        tool_with_verify(
            "type_text",
            "Type text using the keyboard. Simulates real keyboard input character by character. Falls back to SendInput Unicode on Windows for non-Latin characters. Use click_screen first to focus the target input field.",
            &[
                p("text", "string", "The text to type"),
                p("screenshot", "boolean", "Take a screenshot after typing (default: true)"),
                p("delay_ms", "integer", "Milliseconds to wait after typing before screenshot (default: 300)"),
            ],
            &[
                p("retry", "integer", "Retry count 0-3 on failure (default 0)"),
                p("timeout_ms", "integer", "Operation timeout in ms (1000-60000, default 20000)"),
            ],
            &["text"],
        ),
        // ─── press_key (tool 21) — has verification params ───
        tool_with_verify(
            "press_key",
            "Press a key or key combination. Supports modifiers (ctrl, alt, shift, meta/win) and special keys (enter, tab, escape, backspace, delete, up, down, left, right, home, end, pageup, pagedown, f1-f12, space). For combinations use '+': 'ctrl+c', 'ctrl+shift+s', 'alt+tab', 'alt+f4'.",
            &[
                p("key", "string", "Key or key combination. Examples: 'enter', 'tab', 'ctrl+c', 'ctrl+shift+s', 'alt+tab', 'f5'"),
                p("screenshot", "boolean", "Take a screenshot after key press (default: true)"),
                p("delay_ms", "integer", "Milliseconds to wait after key press before screenshot (default: 500)"),
            ],
            &[
                p("retry", "integer", "Retry count 0-3 on failure (default 0)"),
                p("timeout_ms", "integer", "Operation timeout in ms (1000-60000, default 20000)"),
            ],
            &["key"],
        ),
        // ─── scroll_screen — has verification params + extra fields ───
        tool_with_verify(
            "scroll_screen",
            "Scroll the mouse wheel at the current or specified position. Positive amount scrolls down, negative scrolls up. Each unit is about 3 lines of text.",
            &[
                p("amount", "integer", "Scroll amount: positive = down, negative = up. Each unit is ~3 lines."),
                p("x", "integer", "X coordinate to scroll at (optional, uses current position if omitted)"),
                p("y", "integer", "Y coordinate to scroll at (optional, uses current position if omitted)"),
                p("horizontal", "boolean", "Scroll horizontally instead of vertically (default: false)"),
                p("screenshot", "boolean", "Take a screenshot after scrolling (default: true)"),
                p("delay_ms", "integer", "Milliseconds to wait after scrolling before screenshot (default: 300)"),
            ],
            &[
                p("mode", "string", "'amount' (default) or 'to_text' (scroll until text appears via OCR)"),
                p("text", "string", "Text to find when mode='to_text'"),
                p("max_scrolls", "integer", "Max scroll attempts for to_text mode (default 20)"),
                p("snap_to_screen", "boolean", "Clamp off-screen coordinates to nearest monitor edge"),
                p("dpi_aware", "boolean", "Apply DPI scaling to coordinates"),
            ],
            &["amount"],
        ),
        // ─── mouse_drag — has verification params ───
        tool_with_verify(
            "mouse_drag",
            "Click and drag the mouse from one position to another. Useful for resizing windows, selecting text, moving objects, or drawing.",
            &[
                p("from_x", "integer", "Starting X coordinate (pixels from left edge)"),
                p("from_y", "integer", "Starting Y coordinate (pixels from top edge)"),
                p("to_x", "integer", "Ending X coordinate"),
                p("to_y", "integer", "Ending Y coordinate"),
                p("button", "string", "Mouse button to use: left (default) or right"),
                p("screenshot", "boolean", "Take a screenshot after dragging (default: true). Set to false to avoid embedding a full-screen capture in the result."),
                p("delay_ms", "integer", "Milliseconds to wait after drag before screenshot (default: 500)"),
            ],
            &[
                p("steps", "integer", "Intermediate points for smooth drag (1=instant, max 100). Increase for drawing or slider control."),
                p("snap_to_screen", "boolean", "Clamp off-screen coordinates to nearest monitor edge"),
                p("timeout_ms", "integer", "Operation timeout in ms (1000-60000, default 20000)"),
            ],
            &["from_x", "from_y", "to_x", "to_y"],
        ),
        // ─── click_window_relative — has verification params ───
        tool_with_verify(
            "click_window_relative",
            "Click at coordinates relative to a window's top-left corner. Focuses the window first. Prefer pid when you already know the target window identity.",
            &[
                p("title", "string", "Window title or process name to match"),
                p("pid", "integer", "Specific process ID to target. Prefer this once you know the window identity."),
                p("x", "integer", "X offset from window's left edge"),
                p("y", "integer", "Y offset from window's top edge"),
                p("button", "string", "Mouse button: left, right, middle, double (default: left)"),
                p("screenshot", "boolean", "Take a screenshot after clicking (default: true). Set to false to avoid embedding a full-screen capture in the result."),
                p("delay_ms", "integer", "Delay before screenshot in ms (default 500)"),
            ],
            &[],
            &["x", "y"],
        ),
        // ─── get_ui_tree — has array param (exclude_types) ───
        json!({
            "name": "get_ui_tree",
            "description": "Get the UI element tree of a window using UI Automation. Shows control types and names. Works best with native Windows apps (Win32, WPF, WinForms). Returns empty for GPU-rendered apps (Blender, Unity, games, Electron) — use ocr_screen or take_screenshot instead for those.",
            "parameters": {
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "Window title or process name (default: active window)" },
                    "depth": { "type": "integer", "description": "Max tree depth 1-8 (default 3)" },
                    "exclude_types": { "type": "array", "items": { "type": "string" }, "description": "Control types to exclude (e.g. ['image','separator','thumb'])" }
                },
                "required": []
            }
        }),
        // ─── fill_form — has complex array param ───
        json!({
            "name": "fill_form",
            "description": "Fill multiple form fields by finding UI elements by label and typing values. Each field is clicked, cleared, and filled.",
            "parameters": {
                "type": "object",
                "properties": {
                    "fields": {
                        "type": "array",
                        "description": "Array of {label, value} objects. Each field object can include \"type\": \"text|checkbox|dropdown|radio\" to force field type handling.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "label": { "type": "string" },
                                "value": { "type": "string" },
                                "type": { "type": "string", "description": "Force field type: text, checkbox, dropdown, or radio" }
                            }
                        }
                    },
                    "title": { "type": "string", "description": "Window title filter" }
                },
                "required": ["fields"]
            }
        }),
        // ─── run_action_sequence — has complex array param ───
        json!({
            "name": "run_action_sequence",
            "description": "Execute a sequence of desktop actions (click, type, press_key, paste, wait, clear, scroll, move). Each action is a JSON object with an 'action' field.",
            "parameters": {
                "type": "object",
                "properties": {
                    "actions": {
                        "type": "array",
                        "description": "Array of action objects. Each has 'action' (click/type/press_key/paste/wait/clear/scroll/move) plus params. Per-action options: 'retry' (0-3), 'if_previous' ('success'|'failure'), 'abort_on_failure' (boolean), 'screenshot_mode' ('final_only'|'all'|'none').",
                        "items": { "type": "object" }
                    },
                    "delay_between_ms": { "type": "integer", "description": "Default delay between actions (default 200)" }
                },
                "required": ["actions"]
            }
        }),
        // ─── annotate_screenshot — has complex array param ───
        json!({
            "name": "annotate_screenshot",
            "description": "Draw shapes (rectangles, circles, lines) on a screenshot for visual annotation.",
            "parameters": {
                "type": "object",
                "properties": {
                    "shapes": {
                        "type": "array",
                        "description": "Array of shapes: {type: rect|circle|line, x, y, w, h, r, x1, y1, x2, y2, color, thickness}",
                        "items": { "type": "object" }
                    },
                    "monitor": { "type": "integer", "description": "Monitor index (default 0)" }
                },
                "required": ["shapes"]
            }
        }),
        // ─── clipboard_file_paths — has array param ───
        json!({
            "name": "clipboard_file_paths",
            "description": "Read or write file paths on the clipboard (e.g. copied files in a file manager).",
            "parameters": {
                "type": "object",
                "properties": {
                    "action": { "type": "string", "description": "'read' to get file paths from clipboard, 'write' to put file paths on clipboard" },
                    "paths": { "type": "array", "description": "File paths to write (required for action='write')" }
                },
                "required": ["action"]
            }
        }),
        // ─── clipboard_html ───
        json!({
            "name": "clipboard_html",
            "description": "Read or write HTML content on the clipboard.",
            "parameters": {
                "type": "object",
                "properties": {
                    "action": { "type": "string", "description": "'read' to get HTML from clipboard, 'write' to put HTML on clipboard" },
                    "html": { "type": "string", "description": "HTML content to write (required for action='write')" }
                },
                "required": ["action"]
            }
        }),
        // ─── add_mcp_server — has array and object params ───
        json!({
            "name": "add_mcp_server",
            "description": "Add a new MCP server to extend your capabilities with external tools. Supports stdio (command-based) and http transports. New tools become available in the next message.",
            "parameters": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Display name for the MCP server" },
                    "transport": { "type": "string", "description": "Transport type: 'stdio' (default) or 'http'" },
                    "command": { "type": "string", "description": "Command to run (required for stdio transport, e.g. 'npx', 'uvx', 'node')" },
                    "args": { "type": "array", "items": { "type": "string" }, "description": "Command arguments (for stdio transport, e.g. ['-y', '@anthropic/mcp-server'])" },
                    "url": { "type": "string", "description": "Server URL (required for http transport)" },
                    "env_vars": { "type": "object", "description": "Environment variables to set for the server process (e.g. {\"API_KEY\": \"xxx\"})" }
                },
                "required": ["name"]
            }
        }),
        // ─── dialog_handler_start — has object param ───
        json!({
            "name": "dialog_handler_start",
            "description": "Start a background monitor that auto-clicks dialog buttons matching a button map. Useful for dismissing expected popups during automated workflows.",
            "parameters": {
                "type": "object",
                "properties": {
                    "button_map": { "type": "object", "description": "Map of button names to actions, e.g. {\"OK\": \"click\", \"Cancel\": \"click\"}" },
                    "poll_interval_ms": { "type": "integer", "description": "Polling interval in ms (default 1000)" },
                    "timeout_ms": { "type": "integer", "description": "Auto-stop after this many ms (default 60000)" }
                },
                "required": ["button_map"]
            }
        }),
    ]
}

/// Build the complete list of all tool definitions.
///
/// Merges the compact static definitions with the complex runtime-built ones.
/// The complex tools override any same-named tool from the static list (but
/// in practice there's no overlap — `clipboard_html` is the only one that
/// could be in both, and it's only in the complex list).
pub fn all_tool_definitions() -> Vec<Value> {
    let mut tools: Vec<Value> = ALL_TOOLS.iter().map(|t| t.to_json()).collect();
    let complex = complex_tool_definitions();

    // Collect names of complex tools so we can deduplicate
    let complex_names: Vec<&str> = complex
        .iter()
        .filter_map(|t| t.get("name").and_then(|n| n.as_str()))
        .collect();

    // Remove any simple-list tools that are overridden by complex versions
    tools.retain(|t| {
        let name = t.get("name").and_then(|n| n.as_str()).unwrap_or("");
        !complex_names.contains(&name)
    });

    tools.extend(complex);
    tools
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_tool_count() {
        let tools = all_tool_definitions();
        assert_eq!(
            tools.len(),
            EXPECTED_TOOL_COUNT,
            "Expected {} tools, got {}. Tool names: {:?}",
            EXPECTED_TOOL_COUNT,
            tools.len(),
            tools.iter().filter_map(|t| t.get("name").and_then(|n| n.as_str())).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_no_duplicate_names() {
        let tools = all_tool_definitions();
        let mut seen = HashSet::new();
        for tool in &tools {
            let name = tool.get("name").and_then(|n| n.as_str()).unwrap();
            assert!(seen.insert(name), "Duplicate tool name: {}", name);
        }
    }

    #[test]
    fn test_all_tools_have_required_fields() {
        let tools = all_tool_definitions();
        for tool in &tools {
            assert!(tool.get("name").is_some(), "Tool missing name: {:?}", tool);
            assert!(tool.get("description").is_some(), "Tool missing description: {:?}", tool);
            assert!(tool.get("parameters").is_some(), "Tool missing parameters: {:?}", tool);
            let params = tool.get("parameters").unwrap();
            assert_eq!(params.get("type").and_then(|t| t.as_str()), Some("object"));
            assert!(params.get("properties").is_some());
            assert!(params.get("required").is_some());
        }
    }

    #[test]
    fn test_click_screen_has_verify_params() {
        let tools = all_tool_definitions();
        let click = tools.iter().find(|t| t["name"] == "click_screen").unwrap();
        let props = click["parameters"]["properties"].as_object().unwrap();
        assert!(props.contains_key("verify_screen_change"));
        assert!(props.contains_key("verify_text"));
        assert!(props.contains_key("x"));
        assert!(props.contains_key("y"));
    }
}
