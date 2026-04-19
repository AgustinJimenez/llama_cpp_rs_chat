//! Camofox browser backend — anti-detection Firefox via REST API.
//!
//! Connects to a camofox-browser server (Camoufox) for bot-resistant web browsing.
//! The server is bundled as a Tauri sidecar (standalone binary, no Node.js needed).
//!
//! On first use, the Camoufox browser binary (~300MB) is downloaded automatically.
//! The download status is exposed via `get_status()` so the frontend can show progress.

use serde_json::Value;
use std::io::{BufRead, BufReader, Read};
use std::path::PathBuf;
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

/// Default Camofox server port.
const DEFAULT_PORT: u16 = 9377;

/// User ID for all tabs created by this app.
const USER_ID: &str = "llama-chat";

/// Maximum wait time for Camofox server to become healthy after starting.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(300); // 5min for first-run download

/// Maximum wait time for individual HTTP requests to Camofox.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Managed Camofox child process.
static MANAGED_PROCESS: Mutex<Option<Child>> = Mutex::new(None);

/// Active CAPTCHA tab — kept open so the user/model can interact with it.
static CAPTCHA_TAB: Mutex<Option<String>> = Mutex::new(None);

/// Agent-requested browser view tab — opened via `open_browser_view` tool,
/// shown to the user in the in-app browser view.
static AGENT_BROWSER_TAB: Mutex<Option<(String, String)>> = Mutex::new(None); // (tab_id, url)

/// Current status message (for frontend progress display).
static STATUS_MESSAGE: Mutex<Option<String>> = Mutex::new(None);

/// Whether the Camoufox browser binary is being downloaded.
static DOWNLOADING: AtomicBool = AtomicBool::new(false);

// ── Status API ─────────────────────────────────────────────────────

/// Status info returned to the frontend.
#[derive(serde::Serialize, Clone)]
pub struct CamofoxStatus {
    pub available: bool,
    pub healthy: bool,
    pub downloading: bool,
    pub message: Option<String>,
    /// Active CAPTCHA tab ID — frontend can use this to open the browser view.
    pub captcha_tab_id: Option<String>,
    /// Agent-requested browser view tab ID (from `open_browser_view` tool).
    pub agent_tab_id: Option<String>,
    /// URL of the agent-requested tab.
    pub agent_tab_url: Option<String>,
}

/// Get current Camofox status for the frontend.
pub fn get_status() -> CamofoxStatus {
    let msg = STATUS_MESSAGE.lock().ok().and_then(|g| g.clone());
    // Try local CAPTCHA_TAB first (worker process), then query Camofox server (web server process)
    let captcha_tab = get_captcha_tab().or_else(detect_captcha_tab_from_server);
    // Try local AGENT_BROWSER_TAB first; fall back to the most recent
    // non-CAPTCHA tab on the Camofox server (works across processes).
    let (agent_tab_id, agent_tab_url) = AGENT_BROWSER_TAB
        .lock()
        .ok()
        .and_then(|g| g.clone())
        .map(|(id, url)| (Some(id), Some(url)))
        .or_else(|| {
            detect_agent_tab_from_server().map(|(id, url)| (Some(id), Some(url)))
        })
        .unwrap_or((None, None));
    CamofoxStatus {
        available: find_camofox_binary().is_some(),
        healthy: is_healthy(),
        downloading: DOWNLOADING.load(Ordering::Relaxed),
        message: msg,
        captcha_tab_id: captcha_tab,
        agent_tab_id,
        agent_tab_url,
    }
}

/// Find a non-CAPTCHA tab on Camofox — used by the web server process to
/// surface tabs created by the worker process (where AGENT_BROWSER_TAB lives).
fn detect_agent_tab_from_server() -> Option<(String, String)> {
    let resp = agent()
        .get(&format!("{}/tabs?userId={USER_ID}", base_url()))
        .call()
        .ok()?;
    let text = resp.into_string().ok()?;
    let data: Value = serde_json::from_str(&text).ok()?;
    let tabs = data.get("tabs")?.as_array()?;
    // Return the LAST tab that's not a CAPTCHA page (most recently created)
    for tab in tabs.iter().rev() {
        let url = tab.get("url").and_then(|v| v.as_str()).unwrap_or("");
        if url.contains("/sorry") || url.contains("captcha") || url.contains("recaptcha") {
            continue;
        }
        if let Some(id) = extract_tab_id(tab) {
            return Some((id, url.to_string()));
        }
    }
    None
}

/// Query the Camofox server for tabs on Google's CAPTCHA page (google.com/sorry/).
/// This works from the web server process even though CAPTCHA_TAB is in the worker.
fn detect_captcha_tab_from_server() -> Option<String> {
    let resp = agent()
        .get(&format!("{}/tabs?userId={USER_ID}", base_url()))
        .call()
        .ok()?;
    let text = resp.into_string().ok()?;
    let data: Value = serde_json::from_str(&text).ok()?;
    let tabs = data.get("tabs")?.as_array()?;
    for tab in tabs {
        let url = tab.get("url").and_then(|v| v.as_str()).unwrap_or("");
        if url.contains("/sorry") || url.contains("captcha") || url.contains("recaptcha") {
            return extract_tab_id(tab);
        }
    }
    None
}

fn set_status(msg: &str) {
    eprintln!("[CAMOFOX] {msg}");
    if let Ok(mut guard) = STATUS_MESSAGE.lock() {
        *guard = Some(msg.to_string());
    }
}

fn clear_status() {
    if let Ok(mut guard) = STATUS_MESSAGE.lock() {
        *guard = None;
    }
    DOWNLOADING.store(false, Ordering::Relaxed);
}

/// Get the Camofox server base URL.
fn base_url() -> String {
    std::env::var("CAMOFOX_URL")
        .unwrap_or_else(|_| format!("http://localhost:{DEFAULT_PORT}"))
}

/// Build an HTTP agent with timeouts.
fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(5))
        .timeout_read(REQUEST_TIMEOUT)
        .build()
}

// ── Health & Lifecycle ─────────────────────────────────────────────

/// Check if the Camofox server is reachable.
pub fn is_healthy() -> bool {
    agent()
        .get(&format!("{}/health", base_url()))
        .call()
        .map(|r| r.status() == 200)
        .unwrap_or(false)
}

/// Ensure the Camofox server is running. If not, try to auto-start it.
pub fn ensure_running() -> Result<(), String> {
    if is_healthy() {
        return Ok(());
    }

    start_managed()?;

    // Wait for it to become healthy (may take a while on first run due to browser download)
    let start = std::time::Instant::now();
    while start.elapsed() < STARTUP_TIMEOUT {
        if is_healthy() {
            set_status("Camofox server ready");
            clear_status();
            return Ok(());
        }
        std::thread::sleep(Duration::from_secs(2));
    }

    clear_status();
    Err(
        "Camofox server did not become healthy within 5 minutes. \
         Check logs for details."
            .to_string(),
    )
}

/// Try to auto-start camofox-browser as a managed subprocess.
fn start_managed() -> Result<(), String> {
    let mut guard = MANAGED_PROCESS
        .lock()
        .map_err(|e| format!("Lock error: {e}"))?;

    // Already have a child? Check if still alive.
    if let Some(ref mut child) = *guard {
        match child.try_wait() {
            Ok(None) => return Ok(()), // still running
            Ok(Some(status)) => {
                eprintln!("[CAMOFOX] Previous process exited with {status}");
            }
            Err(e) => {
                eprintln!("[CAMOFOX] Error checking process: {e}");
            }
        }
    }

    let binary = find_camofox_binary().ok_or_else(|| {
        "camofox-browser binary not found. \
         Run `node scripts/build-camofox.mjs` to build it, \
         or set CAMOFOX_URL to point to a running instance."
            .to_string()
    })?;

    set_status("Starting Camofox server...");

    let mut cmd = std::process::Command::new(&binary);

    // If it's npx, add the package name
    if binary.contains("npx") {
        cmd.arg("@askjo/camofox-browser");
    }

    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // On Windows, prevent visible console window
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to start camofox-browser: {e}"))?;

    let pid = child.id();
    eprintln!("[CAMOFOX] Process spawned (pid={pid})");

    // Monitor stderr in a background thread for download progress
    if let Some(stderr) = child.stderr.take() {
        std::thread::Builder::new()
            .name("camofox-stderr".into())
            .spawn(move || {
                let reader = BufReader::new(stderr);
                for line in reader.lines() {
                    match line {
                        Ok(line) => {
                            // Parse structured JSON logs from camofox-browser
                            if let Ok(log) = serde_json::from_str::<Value>(&line) {
                                let msg = log
                                    .get("msg")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("");
                                // Detect download progress messages
                                if msg.contains("download")
                                    || msg.contains("fetch")
                                    || msg.contains("install")
                                {
                                    DOWNLOADING.store(true, Ordering::Relaxed);
                                    set_status(&format!(
                                        "Downloading Camoufox browser: {msg}"
                                    ));
                                } else if msg.contains("ready") || msg.contains("listening") {
                                    clear_status();
                                }
                            }
                            eprintln!("[CAMOFOX:stderr] {line}");
                        }
                        Err(_) => break,
                    }
                }
            })
            .ok();
    }

    *guard = Some(child);
    Ok(())
}

/// Find the camofox-browser binary. Search order:
/// 1. CAMOFOX_BIN env var
/// 2. Bundled Tauri sidecar (binaries/camofox-server{-target}{.exe})
/// 3. Global npm install (camofox-browser in PATH)
/// 4. npx fallback
fn find_camofox_binary() -> Option<String> {
    // 1. Explicit env var
    if let Ok(path) = std::env::var("CAMOFOX_BIN") {
        if std::path::Path::new(&path).exists() {
            return Some(path);
        }
    }

    // 2. Bundled sidecar — next to our own executable
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            // Try platform-specific sidecar names
            for name in sidecar_names() {
                let candidate = dir.join(&name);
                if candidate.exists() {
                    return Some(candidate.to_string_lossy().to_string());
                }
            }
            // Also check binaries/ subdirectory (dev layout)
            for name in sidecar_names() {
                let candidate = dir.join("binaries").join(&name);
                if candidate.exists() {
                    return Some(candidate.to_string_lossy().to_string());
                }
            }
        }
    }

    // Also check project root binaries/ dir (for development)
    let project_binaries = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("binaries");
    if project_binaries.exists() {
        for name in sidecar_names() {
            let candidate = project_binaries.join(&name);
            if candidate.exists() {
                return Some(candidate.to_string_lossy().to_string());
            }
        }
    }

    // 3. Check PATH
    let bin_name = if cfg!(target_os = "windows") {
        "camofox-browser.cmd"
    } else {
        "camofox-browser"
    };

    #[cfg(target_os = "windows")]
    let which_cmd = "where";
    #[cfg(not(target_os = "windows"))]
    let which_cmd = "which";

    if let Ok(output) = std::process::Command::new(which_cmd)
        .arg(bin_name)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
    {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout)
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .to_string();
            if !path.is_empty() {
                return Some(path);
            }
        }
    }

    // 4. npx fallback
    #[cfg(target_os = "windows")]
    let npx = "npx.cmd";
    #[cfg(not(target_os = "windows"))]
    let npx = "npx";

    let npx_check = std::process::Command::new(npx)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    if npx_check.map(|s| s.success()).unwrap_or(false) {
        return Some(npx.to_string());
    }

    None
}

/// Generate sidecar binary names for the current platform.
fn sidecar_names() -> Vec<String> {
    let ext = if cfg!(target_os = "windows") {
        ".exe"
    } else {
        ""
    };

    let target = if cfg!(target_os = "windows") {
        "x86_64-pc-windows-msvc"
    } else if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") {
            "aarch64-apple-darwin"
        } else {
            "x86_64-apple-darwin"
        }
    } else if cfg!(target_arch = "aarch64") {
        "aarch64-unknown-linux-gnu"
    } else {
        "x86_64-unknown-linux-gnu"
    };

    vec![
        format!("camofox-server-{target}{ext}"),
        format!("camofox-server{ext}"),
    ]
}

/// Kill the managed Camofox process.
pub fn shutdown() {
    if let Ok(mut guard) = MANAGED_PROCESS.lock() {
        if let Some(ref mut child) = *guard {
            eprintln!(
                "[CAMOFOX] Shutting down managed process (pid={})",
                child.id()
            );
            let _ = child.kill();
            let _ = child.wait();
        }
        *guard = None;
    }
    clear_status();
}

// ── Tab Operations ─────────────────────────────────────────────────

/// Create a new tab navigating to the given URL. Returns the tab ID.
fn create_tab(url: &str) -> Result<String, String> {
    let body = serde_json::json!({
        "userId": USER_ID,
        "sessionKey": "default",
        "url": url,
    });

    let resp = agent()
        .post(&format!("{}/tabs", base_url()))
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
        .map_err(|e| format!("Camofox create tab failed: {e}"))?;

    let text = resp
        .into_string()
        .map_err(|e| format!("Read response: {e}"))?;
    let data: Value =
        serde_json::from_str(&text).map_err(|e| format!("Parse response: {e}"))?;

    extract_tab_id(&data).ok_or_else(|| format!("No tab ID in response: {text}"))
}

/// Create a tab using a search macro (e.g. @google_search).
#[allow(dead_code)]
fn create_tab_with_macro(macro_name: &str, query: &str) -> Result<String, String> {
    let body = serde_json::json!({
        "userId": USER_ID,
        "sessionKey": "default",
        "macro": macro_name,
        "query": query,
    });

    let resp = agent()
        .post(&format!("{}/tabs", base_url()))
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
        .map_err(|e| format!("Camofox macro tab failed: {e}"))?;

    let text = resp
        .into_string()
        .map_err(|e| format!("Read response: {e}"))?;
    let data: Value =
        serde_json::from_str(&text).map_err(|e| format!("Parse response: {e}"))?;

    extract_tab_id(&data).ok_or_else(|| format!("No tab ID in response: {text}"))
}

/// Navigate an existing tab using a macro.
#[allow(dead_code)]
fn navigate_with_macro(tab_id: &str, macro_name: &str, query: &str) -> Result<(), String> {
    let body = serde_json::json!({
        "userId": USER_ID,
        "macro": macro_name,
        "query": query,
    });

    agent()
        .post(&format!("{}/tabs/{tab_id}/navigate", base_url()))
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
        .map_err(|e| format!("Camofox navigate failed: {e}"))?;

    Ok(())
}

/// Get the accessibility snapshot of a tab.
fn get_snapshot(tab_id: &str) -> Result<String, String> {
    let resp = agent()
        .get(&format!(
            "{}/tabs/{tab_id}/snapshot?userId={USER_ID}",
            base_url()
        ))
        .call()
        .map_err(|e| format!("Camofox snapshot failed: {e}"))?;

    let text = resp
        .into_string()
        .map_err(|e| format!("Read snapshot: {e}"))?;
    let data: Value = serde_json::from_str(&text).unwrap_or(Value::String(text.clone()));

    if let Some(snap) = data.get("snapshot").and_then(|v| v.as_str()) {
        Ok(snap.to_string())
    } else if let Some(snap) = data.get("content").and_then(|v| v.as_str()) {
        Ok(snap.to_string())
    } else if data.is_string() {
        Ok(data.as_str().unwrap_or("").to_string())
    } else {
        Ok(text)
    }
}

/// Close a tab.
fn close_tab(tab_id: &str) {
    let _ = agent()
        .delete(&format!("{}/tabs/{tab_id}", base_url()))
        .call();
}

/// Extract tab ID from a JSON response (handles string or number).
fn extract_tab_id(data: &Value) -> Option<String> {
    for key in &["tabId", "id"] {
        if let Some(v) = data.get(key) {
            if let Some(s) = v.as_str() {
                return Some(s.to_string());
            }
            if let Some(n) = v.as_u64() {
                return Some(n.to_string());
            }
        }
    }
    None
}

// ── Public API (BrowserBackend interface) ──────────────────────────

/// Fetch a web page as plain text via Camofox accessibility snapshot.
pub fn fetch_text(url: &str, max_chars: usize) -> Result<String, String> {
    ensure_running()?;

    let tab_id = create_tab(url)?;
    let snapshot = get_snapshot(&tab_id);
    close_tab(&tab_id);

    let text = snapshot?;

    if text.len() > max_chars {
        let mut t = max_chars;
        while t > 0 && !text.is_char_boundary(t) {
            t -= 1;
        }
        Ok(format!(
            "{}...\n[Truncated: first {} of {} chars]",
            &text[..t],
            t,
            text.len()
        ))
    } else {
        Ok(text)
    }
}

/// Fetch raw HTML via Camofox (returns accessibility snapshot).
pub fn fetch_html(url: &str) -> Result<String, String> {
    fetch_text(url, 500_000)
}

/// Result from a Camofox search — may include a screenshot if CAPTCHA is detected.
pub struct CamofoxSearchResult {
    pub text: String,
    pub screenshot: Option<Vec<u8>>,
    pub captcha_detected: bool,
}

/// Search via Camofox using DuckDuckGo (avoids Google CAPTCHAs).
/// Returns text results, or a CAPTCHA screenshot if blocked.
pub fn search(query: &str, max_results: usize) -> CamofoxSearchResult {
    if let Err(e) = ensure_running() {
        return CamofoxSearchResult {
            text: format!("Error: {e}"),
            screenshot: None,
            captcha_detected: false,
        };
    }

    // Navigate to DuckDuckGo (Google triggers CAPTCHAs too aggressively)
    let search_url = format!(
        "https://duckduckgo.com/?q={}",
        urlencoding::encode(query)
    );
    let tab_id = match create_tab(&search_url) {
        Ok(id) => id,
        Err(e) => {
            return CamofoxSearchResult {
                text: format!("Error: {e}"),
                screenshot: None,
                captcha_detected: false,
            };
        }
    };

    // Wait for page to load
    std::thread::sleep(Duration::from_secs(2));

    let raw = match get_snapshot(&tab_id) {
        Ok(s) => s,
        Err(e) => {
            close_tab(&tab_id);
            return CamofoxSearchResult {
                text: format!("Error getting search results: {e}"),
                screenshot: None,
                captcha_detected: false,
            };
        }
    };

    // Detect CAPTCHA
    let raw_lower = raw.to_lowercase();
    if raw_lower.contains("unusual traffic")
        || raw_lower.contains("captcha")
        || raw_lower.contains("not a robot")
    {
        eprintln!("[CAMOFOX] CAPTCHA detected, taking screenshot for user");

        // Take screenshot to show the user
        let screenshot = take_tab_screenshot(&tab_id);

        // Keep the tab open — store it so the model can interact via camofox_click
        if let Ok(mut guard) = CAPTCHA_TAB.lock() {
            *guard = Some(tab_id);
        }

        return CamofoxSearchResult {
            text: format!(
                "Google is showing a CAPTCHA verification. I've captured a screenshot of it.\n\
                 The page shows: {}\n\n\
                 You can interact with the CAPTCHA using:\n\
                 - camofox_click: click an element (e.g. \"I'm not a robot\" checkbox)\n\
                 - camofox_screenshot: take a new screenshot to see the current state\n\n\
                 After solving the CAPTCHA, retry web_search and it should work.",
                raw.chars().take(500).collect::<String>()
            ),
            screenshot,
            captcha_detected: true,
        };
    }

    // Normal results — close tab and return
    close_tab(&tab_id);

    CamofoxSearchResult {
        text: format_search_results(&raw, query, max_results),
        screenshot: None,
        captcha_detected: false,
    }
}

/// Parse Camofox accessibility snapshot of Google results into numbered format.
fn format_search_results(snapshot: &str, query: &str, max_results: usize) -> String {
    let mut output = String::new();
    let mut count = 0;
    let lines: Vec<&str> = snapshot.lines().collect();
    let mut i = 0;

    while i < lines.len() && count < max_results {
        let line = lines[i].trim();

        if let Some(title) = extract_link_title(line) {
            if !title.is_empty()
                && title.len() > 3
                && !title.contains("Sign in")
                && !title.contains("Google")
                && !title.to_lowercase().contains("cookie")
            {
                count += 1;
                output.push_str(&format!("{count}. {title}\n"));

                for j in 1..=5 {
                    if i + j >= lines.len() {
                        break;
                    }
                    let next = lines[i + j].trim();
                    if let Some(url) = extract_url_from_line(next) {
                        output.push_str(&format!("   URL: {url}\n"));
                    } else if next.len() > 20 && !next.starts_with('[') {
                        let desc = next
                            .trim_start_matches(|c: char| c == '"' || c == ' ')
                            .trim_end_matches('"');
                        if !desc.is_empty() {
                            output.push_str(&format!("   {desc}\n"));
                            break;
                        }
                    }
                }
                output.push('\n');
            }
        }

        i += 1;
    }

    if output.is_empty() {
        let max = 8000.min(snapshot.len());
        let mut end = max;
        while end > 0 && !snapshot.is_char_boundary(end) {
            end -= 1;
        }
        return format!(
            "Search results for '{query}' (via Camofox):\n\n{}\n\
             Note: Use web_fetch to read specific URLs for more detail.",
            &snapshot[..end]
        );
    }

    format!(
        "Search results for '{query}' (via Camofox/DuckDuckGo):\n\n{output}\
         Note: Use web_fetch to read specific URLs for more detail."
    )
}

/// Extract a link title from an accessibility snapshot line.
fn extract_link_title(line: &str) -> Option<String> {
    if let Some(pos) = line.find("link \"") {
        let start = pos + 6;
        let rest = &line[start..];
        if let Some(end) = rest.rfind('"') {
            let title = &rest[..end];
            if !title.is_empty() {
                return Some(title.to_string());
            }
        }
    }
    None
}

/// Extract a URL from a snapshot line.
fn extract_url_from_line(line: &str) -> Option<String> {
    for prefix in &["https://", "http://"] {
        if let Some(pos) = line.find(prefix) {
            let rest = &line[pos..];
            let url_end = rest
                .find(|c: char| c.is_whitespace() || c == '"' || c == '\'')
                .unwrap_or(rest.len());
            let url = &rest[..url_end];
            if url.len() > 10 {
                return Some(url.to_string());
            }
        }
    }
    None
}

// ── Generic browser session API (used by unified `browser_*` tools) ──

/// Click an element by CSS selector.
pub fn cf_click_selector(tab_id: &str, selector: &str) -> Result<(), String> {
    let body = serde_json::json!({ "userId": USER_ID, "selector": selector });
    agent()
        .post(&format!("{}/tabs/{tab_id}/click", base_url()))
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
        .map_err(|e| format!("click failed: {e}"))?;
    Ok(())
}

/// Type text into an element by CSS selector.
pub fn cf_type_selector(
    tab_id: &str,
    selector: &str,
    text: &str,
    press_enter: bool,
) -> Result<(), String> {
    let body = serde_json::json!({
        "userId": USER_ID,
        "selector": selector,
        "text": text,
        "pressEnter": press_enter,
    });
    agent()
        .post(&format!("{}/tabs/{tab_id}/type", base_url()))
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
        .map_err(|e| format!("type failed: {e}"))?;
    Ok(())
}

/// Evaluate JavaScript in the page context. Returns the JSON result.
pub fn cf_eval(tab_id: &str, expression: &str) -> Result<Value, String> {
    let body = serde_json::json!({ "userId": USER_ID, "expression": expression });
    let resp = agent()
        .post(&format!("{}/tabs/{tab_id}/evaluate", base_url()))
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
        .map_err(|e| format!("evaluate failed: {e}"))?;
    let text = resp.into_string().map_err(|e| format!("read response: {e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("parse response: {e}"))
}

/// Get the full page HTML via evaluate.
pub fn cf_get_html(tab_id: &str) -> Result<String, String> {
    let v = cf_eval(tab_id, "document.documentElement.outerHTML")?;
    Ok(v.get("result")
        .and_then(|r| r.as_str())
        .unwrap_or("")
        .to_string())
}

/// Wait for a CSS selector to appear in the page.
pub fn cf_wait_selector(tab_id: &str, selector: &str, timeout_ms: u64) -> Result<bool, String> {
    let js = format!(
        r#"new Promise((resolve) => {{
            const t0 = Date.now();
            const check = () => {{
                if (document.querySelector({sel_lit})) return resolve(true);
                if (Date.now() - t0 >= {timeout}) return resolve(false);
                setTimeout(check, 100);
            }};
            check();
        }})"#,
        sel_lit = serde_json::to_string(selector).unwrap_or_else(|_| "''".to_string()),
        timeout = timeout_ms,
    );
    let v = cf_eval(tab_id, &js)?;
    Ok(v.get("result").and_then(|r| r.as_bool()).unwrap_or(false))
}

/// Navigate an existing tab to a new URL.
pub fn cf_navigate(tab_id: &str, url: &str) -> Result<(), String> {
    let body = serde_json::json!({ "userId": USER_ID, "url": url });
    agent()
        .post(&format!("{}/tabs/{tab_id}/navigate", base_url()))
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
        .map_err(|e| format!("navigate failed: {e}"))?;
    Ok(())
}

/// Press a keyboard key in the tab. Uses Playwright's `page.keyboard.press()`
/// which produces real browser events (works in iframes, focused inputs, etc.).
pub fn cf_press_key(tab_id: &str, key: &str) -> Result<(), String> {
    let body = serde_json::json!({ "userId": USER_ID, "key": key });
    agent()
        .post(&format!("{}/tabs/{tab_id}/key", base_url()))
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
        .map_err(|e| format!("key failed: {e}"))?;
    Ok(())
}

/// Get the accessibility snapshot of the page — a structured list of
/// interactable elements with refs, labels, types. Much smaller than HTML.
pub fn cf_snapshot(tab_id: &str) -> Result<String, String> {
    let resp = agent()
        .get(&format!("{}/tabs/{tab_id}/snapshot?userId={USER_ID}", base_url()))
        .call()
        .map_err(|e| format!("snapshot failed: {e}"))?;
    let text = resp.into_string().map_err(|e| format!("read snapshot: {e}"))?;
    let data: Value = serde_json::from_str(&text).unwrap_or(Value::String(text.clone()));
    Ok(data.get("snapshot")
        .and_then(|v| v.as_str())
        .unwrap_or(&text)
        .to_string())
}

/// Get the active agent browser tab — created by `open_browser_view` or implicitly
/// when the agent calls a `browser_*` tool. Returns (tab_id, url).
pub fn get_agent_tab() -> Option<(String, String)> {
    AGENT_BROWSER_TAB.lock().ok().and_then(|g| g.clone())
}

/// Set the active agent browser tab. Closes the previous one if any.
pub fn set_agent_tab(tab_id: String, url: String) {
    if let Ok(mut guard) = AGENT_BROWSER_TAB.lock() {
        if let Some((prev_id, _)) = guard.take() {
            close_tab(&prev_id);
        }
        *guard = Some((tab_id, url));
    }
}

/// Clear and close the active agent browser tab.
pub fn clear_agent_tab() {
    if let Ok(mut guard) = AGENT_BROWSER_TAB.lock() {
        if let Some((tab_id, _)) = guard.take() {
            close_tab(&tab_id);
        }
    }
}

/// Public wrapper around the internal `create_tab` for use by the session abstraction.
pub fn cf_create_tab(url: &str) -> Result<String, String> {
    ensure_running()?;
    create_tab(url)
}

// ── Frontend Proxy API ────────────────────────────────────────────

/// Proxy a screenshot request from the frontend to Camofox.
/// Returns raw PNG bytes.
pub fn proxy_screenshot(tab_id: &str) -> Result<Vec<u8>, String> {
    if tab_id.is_empty() {
        // No specific tab — use the CAPTCHA tab if one is active
        let captcha_id = get_captcha_tab()
            .ok_or("No tab ID provided and no active CAPTCHA tab")?;
        return proxy_screenshot(&captcha_id);
    }

    take_tab_screenshot(tab_id)
        .ok_or_else(|| "Failed to capture screenshot".to_string())
}

/// Proxy tab creation from the frontend to Camofox.
/// Body: `{"url": "..."}`. Returns: `{"tabId": "..."}`.
pub fn proxy_create_tab(body: &str) -> Result<String, String> {
    ensure_running()?;

    let data: Value = serde_json::from_str(body)
        .map_err(|e| format!("Invalid JSON: {e}"))?;
    let url = data.get("url").and_then(|v| v.as_str())
        .ok_or("Missing 'url' field")?;

    eprintln!("[CAMOFOX] Creating tab for {url}");

    let tab_id = create_tab(url)?;
    Ok(serde_json::json!({ "tabId": tab_id }).to_string())
}

/// Proxy a click from the frontend to Camofox.
/// Body should be JSON with `x` and `y` coordinates (pixel coords on the page).
///
/// Tries two strategies:
/// 1. JavaScript mouse event dispatch (works for same-origin elements)
/// 2. Camofox ref-based click (for accessibility-visible elements)
pub fn proxy_click(tab_id: &str, body: &str) -> Result<String, String> {
    let data: Value = serde_json::from_str(body)
        .map_err(|e| format!("Invalid JSON: {e}"))?;

    let x = data.get("x").and_then(|v| v.as_f64())
        .ok_or("Missing 'x' coordinate")?;
    let y = data.get("y").and_then(|v| v.as_f64())
        .ok_or("Missing 'y' coordinate")?;

    let tab = if tab_id.is_empty() {
        get_captcha_tab().ok_or("No active tab")?
    } else {
        tab_id.to_string()
    };

    eprintln!("[CAMOFOX] Proxy click at ({x}, {y}) on tab {tab}");

    // Use Playwright's page.mouse.click() via the /mouse-click endpoint.
    // This generates real browser-level mouse events that propagate into
    // iframes (like reCAPTCHA), unlike synthetic JS events.
    let click_body = serde_json::json!({
        "userId": USER_ID,
        "x": x,
        "y": y,
    });

    let resp = agent()
        .post(&format!("{}/tabs/{tab}/mouse-click", base_url()))
        .set("Content-Type", "application/json")
        .send_string(&click_body.to_string())
        .map_err(|e| format!("Mouse click failed: {e}"))?;

    let text = resp.into_string()
        .map_err(|e| format!("Read response: {e}"))?;

    eprintln!("[CAMOFOX] Click result: {text}");
    Ok(text)
}

/// Get the active CAPTCHA tab ID — exposed for the frontend to know which tab to interact with.
#[allow(dead_code)]
pub fn get_active_tab_id() -> Option<String> {
    get_captcha_tab()
}

// ── Screenshot & CAPTCHA Interaction ───────────────────────────────

/// Take a JPEG screenshot of a tab for fast streaming (used by screencast WS).
pub fn take_tab_screenshot_jpeg(tab_id: &str, quality: u8) -> Option<Vec<u8>> {
    let q = quality.clamp(10, 95);
    let resp = agent()
        .get(&format!(
            "{}/tabs/{tab_id}/screenshot?userId={USER_ID}&type=jpeg&quality={q}",
            base_url()
        ))
        .call()
        .ok()?;
    let mut bytes = Vec::new();
    resp.into_reader().read_to_end(&mut bytes).ok()?;
    if bytes.len() > 4 && bytes[0] == 0xFF && bytes[1] == 0xD8 {
        Some(bytes)
    } else {
        eprintln!(
            "[CAMOFOX] JPEG screenshot returned non-JPEG bytes ({} bytes)",
            bytes.len()
        );
        None
    }
}

/// Take a screenshot of a tab. Returns raw PNG bytes or None.
fn take_tab_screenshot(tab_id: &str) -> Option<Vec<u8>> {
    let resp = agent()
        .get(&format!(
            "{}/tabs/{tab_id}/screenshot?userId={USER_ID}",
            base_url()
        ))
        .call()
        .ok()?;

    // Camofox returns raw PNG bytes directly (not JSON)
    let mut bytes = Vec::new();
    resp.into_reader()
        .read_to_end(&mut bytes)
        .ok()?;

    // Verify it's actually a PNG (starts with \x89PNG)
    if bytes.len() > 8 && bytes[0] == 0x89 && bytes[1] == b'P' {
        Some(bytes)
    } else {
        eprintln!(
            "[CAMOFOX] Screenshot response was not PNG ({} bytes, starts with {:?})",
            bytes.len(),
            &bytes[..bytes.len().min(4)]
        );
        None
    }
}

/// Get the active CAPTCHA tab ID (if one is pending).
fn get_captcha_tab() -> Option<String> {
    CAPTCHA_TAB.lock().ok().and_then(|g| g.clone())
}

/// Clear the CAPTCHA tab (after solving or abandoning).
fn clear_captcha_tab() {
    if let Ok(mut guard) = CAPTCHA_TAB.lock() {
        if let Some(ref tab_id) = *guard {
            close_tab(tab_id);
        }
        *guard = None;
    }
}

/// Click an element ref on the active CAPTCHA tab.
/// Called by the `camofox_click` native tool.
pub fn tool_camofox_click(args: &Value) -> super::super::native_tools::NativeToolResult {
    let element_ref = args
        .get("ref")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if element_ref.is_empty() {
        return super::super::native_tools::NativeToolResult::text_only(
            "Error: 'ref' argument is required (e.g. \"e1\", \"e3\")".to_string(),
        );
    }

    let tab_id = match get_captcha_tab() {
        Some(id) => id,
        None => {
            return super::super::native_tools::NativeToolResult::text_only(
                "Error: No active Camofox tab. Use web_search first.".to_string(),
            );
        }
    };

    // Click the element
    let body = serde_json::json!({
        "userId": USER_ID,
        "ref": element_ref,
    });

    if let Err(e) = agent()
        .post(&format!("{}/tabs/{tab_id}/click", base_url()))
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
    {
        return super::super::native_tools::NativeToolResult::text_only(
            format!("Error clicking element: {e}"),
        );
    }

    // Wait for page to update after click
    std::thread::sleep(Duration::from_millis(1500));

    // Take a screenshot to show the result
    let screenshot = take_tab_screenshot(&tab_id);

    // Get updated snapshot for context
    let snapshot_text = get_snapshot(&tab_id).unwrap_or_default();

    // Check if CAPTCHA is solved (no more CAPTCHA text)
    let lower = snapshot_text.to_lowercase();
    let solved = !lower.contains("unusual traffic")
        && !lower.contains("captcha")
        && !lower.contains("not a robot");

    let text = if solved {
        clear_captcha_tab();
        format!(
            "Clicked element [{element_ref}]. CAPTCHA appears to be solved!\n\
             You can now retry web_search — the session cookies are saved.\n\n\
             Current page content:\n{}",
            snapshot_text.chars().take(1000).collect::<String>()
        )
    } else {
        format!(
            "Clicked element [{element_ref}]. The page updated.\n\
             Current page elements:\n{}",
            snapshot_text.chars().take(1500).collect::<String>()
        )
    };

    match screenshot {
        Some(img) => super::super::native_tools::NativeToolResult::with_image(text, img),
        None => super::super::native_tools::NativeToolResult::text_only(text),
    }
}

/// Take a screenshot of the active CAPTCHA tab.
/// Called by the `camofox_screenshot` native tool.
pub fn tool_camofox_screenshot(_args: &Value) -> super::super::native_tools::NativeToolResult {
    let tab_id = match get_captcha_tab() {
        Some(id) => id,
        None => {
            return super::super::native_tools::NativeToolResult::text_only(
                "Error: No active Camofox tab. Use web_search first.".to_string(),
            );
        }
    };

    let snapshot_text = get_snapshot(&tab_id).unwrap_or_default();
    let screenshot = take_tab_screenshot(&tab_id);

    let text = format!(
        "Current Camofox tab state:\n{}",
        snapshot_text.chars().take(2000).collect::<String>()
    );

    match screenshot {
        Some(img) => super::super::native_tools::NativeToolResult::with_image(text, img),
        None => super::super::native_tools::NativeToolResult::text_only(text),
    }
}

/// Agent tool: open a URL in the in-app browser view (visible to the user).
/// The frontend polls the status endpoint and auto-opens the view when
/// agent_tab_id is set.
pub fn tool_open_browser_view(args: &Value) -> super::super::native_tools::NativeToolResult {
    let url = args.get("url").and_then(|v| v.as_str()).unwrap_or("");
    if url.is_empty() {
        return super::super::native_tools::NativeToolResult::text_only(
            "Error: 'url' argument is required".to_string(),
        );
    }

    if let Err(e) = ensure_running() {
        return super::super::native_tools::NativeToolResult::text_only(
            format!("Error starting Camofox: {e}"),
        );
    }

    // Create a Camofox tab at the URL
    let tab_id = match create_tab(url) {
        Ok(id) => id,
        Err(e) => {
            return super::super::native_tools::NativeToolResult::text_only(
                format!("Error creating browser tab: {e}"),
            );
        }
    };

    // Store it as the agent-requested tab — frontend will auto-open the browser view
    if let Ok(mut guard) = AGENT_BROWSER_TAB.lock() {
        *guard = Some((tab_id.clone(), url.to_string()));
    }

    super::super::native_tools::NativeToolResult::text_only(format!(
        "Opened browser view for {url}. The user can now see the page in the chat interface. \
         They can interact with it by clicking. When done, call close_browser_view."
    ))
}

/// Agent tool: close the in-app browser view.
pub fn tool_close_browser_view(_args: &Value) -> super::super::native_tools::NativeToolResult {
    if let Ok(mut guard) = AGENT_BROWSER_TAB.lock() {
        if let Some((tab_id, _)) = guard.take() {
            close_tab(&tab_id);
        }
    }
    super::super::native_tools::NativeToolResult::text_only(
        "Browser view closed.".to_string(),
    )
}

/// Type text into an element on the active CAPTCHA tab.
/// Called by the `camofox_type` native tool.
pub fn tool_camofox_type(args: &Value) -> super::super::native_tools::NativeToolResult {
    let element_ref = args.get("ref").and_then(|v| v.as_str()).unwrap_or("");
    let text_to_type = args.get("text").and_then(|v| v.as_str()).unwrap_or("");
    let press_enter = args.get("press_enter").and_then(|v| v.as_bool()).unwrap_or(false);

    if element_ref.is_empty() || text_to_type.is_empty() {
        return super::super::native_tools::NativeToolResult::text_only(
            "Error: 'ref' and 'text' arguments are required".to_string(),
        );
    }

    let tab_id = match get_captcha_tab() {
        Some(id) => id,
        None => {
            return super::super::native_tools::NativeToolResult::text_only(
                "Error: No active Camofox tab. Use web_search first.".to_string(),
            );
        }
    };

    let body = serde_json::json!({
        "userId": USER_ID,
        "ref": element_ref,
        "text": text_to_type,
        "pressEnter": press_enter,
    });

    if let Err(e) = agent()
        .post(&format!("{}/tabs/{tab_id}/type", base_url()))
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
    {
        return super::super::native_tools::NativeToolResult::text_only(
            format!("Error typing text: {e}"),
        );
    }

    std::thread::sleep(Duration::from_millis(1000));

    let screenshot = take_tab_screenshot(&tab_id);
    let snapshot_text = get_snapshot(&tab_id).unwrap_or_default();

    let result_text = format!(
        "Typed \"{text_to_type}\" into [{element_ref}]{}.\n\
         Current page:\n{}",
        if press_enter { " and pressed Enter" } else { "" },
        snapshot_text.chars().take(1500).collect::<String>()
    );

    match screenshot {
        Some(img) => super::super::native_tools::NativeToolResult::with_image(result_text, img),
        None => super::super::native_tools::NativeToolResult::text_only(result_text),
    }
}
