//! Chrome Headless Shell backend — lightweight variant of Chrome.
//!
//! Uses the same `headless_chrome` crate but targets the `chrome-headless-shell`
//! binary which uses ~50% less RAM than full Chrome. Falls back to standard Chrome
//! if the binary is not found.

use headless_chrome::{Browser, LaunchOptions, Tab};
use std::ffi::OsStr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::sys_debug;

const IDLE_TIMEOUT_SECS: u64 = 300;
const NAV_TIMEOUT: Duration = Duration::from_secs(15);
const FETCH_HARD_TIMEOUT: Duration = Duration::from_secs(25);

lazy_static::lazy_static! {
    static ref HEADLESS_SHELL_BROWSER: Mutex<Option<Browser>> = Mutex::new(None);
}

static LAST_USED: AtomicU64 = AtomicU64::new(0);

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

/// Try to find chrome-headless-shell binary.
fn find_binary() -> Option<PathBuf> {
    // 1. Explicit env var
    if let Ok(path) = std::env::var("CHROME_HEADLESS_SHELL_PATH") {
        let p = PathBuf::from(&path);
        if p.exists() { return Some(p); }
    }

    // 2. Search chrome-headless-shell/ directory tree (npx @puppeteer/browsers install)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(base) = exe.parent() {
            if let Some(found) = find_in_dir(&base.join("chrome-headless-shell")) {
                return Some(found);
            }
        }
    }
    // Also check cwd
    if let Ok(cwd) = std::env::current_dir() {
        if let Some(found) = find_in_dir(&cwd.join("chrome-headless-shell")) {
            return Some(found);
        }
    }

    // 3. Check PATH via `where` (Windows) or `which` (Unix)
    #[cfg(target_os = "windows")]
    let check = crate::web::utils::silent_command("where").arg("chrome-headless-shell").output();
    #[cfg(not(target_os = "windows"))]
    let check = crate::web::utils::silent_command("which").arg("chrome-headless-shell").output();

    if let Ok(output) = check {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                let p = PathBuf::from(&path);
                if p.exists() { return Some(p); }
            }
        }
    }

    // 4. Platform-specific default locations
    #[cfg(target_os = "windows")]
    {
        let candidates = [
            r"C:\Program Files\Google\Chrome\Application\chrome-headless-shell.exe",
            r"C:\Program Files (x86)\Google\Chrome\Application\chrome-headless-shell.exe",
        ];
        for c in &candidates {
            let p = PathBuf::from(c);
            if p.exists() { return Some(p); }
        }
    }

    #[cfg(target_os = "linux")]
    {
        let candidates = [
            "/usr/bin/chrome-headless-shell",
            "/usr/local/bin/chrome-headless-shell",
        ];
        for c in &candidates {
            let p = PathBuf::from(c);
            if p.exists() { return Some(p); }
        }
    }

    #[cfg(target_os = "macos")]
    {
        let candidates = [
            "/Applications/Google Chrome.app/Contents/MacOS/chrome-headless-shell",
        ];
        for c in &candidates {
            let p = PathBuf::from(c);
            if p.exists() { return Some(p); }
        }
    }

    None
}

/// Recursively search a directory for the chrome-headless-shell binary.
fn find_in_dir(dir: &std::path::Path) -> Option<PathBuf> {
    if !dir.is_dir() { return None; }
    let target = if cfg!(target_os = "windows") {
        "chrome-headless-shell.exe"
    } else {
        "chrome-headless-shell"
    };
    for entry in std::fs::read_dir(dir).ok()? {
        let entry = entry.ok()?;
        let path = entry.path();
        if path.is_file() && path.file_name().map(|n| n == target).unwrap_or(false) {
            return Some(path);
        }
        if path.is_dir() {
            if let Some(found) = find_in_dir(&path) {
                return Some(found);
            }
        }
    }
    None
}

fn get_or_init() -> Result<(), String> {
    let mut guard = HEADLESS_SHELL_BROWSER.lock().unwrap_or_else(|e| e.into_inner());
    let now = now_secs();
    let last = LAST_USED.load(Ordering::Relaxed);

    if guard.is_some() && last > 0 && now.saturating_sub(last) > IDLE_TIMEOUT_SECS {
        sys_debug!("[BROWSER:chrome-headless-shell] Idle timeout, shutting down");
        *guard = None;
    }

    if let Some(ref browser) = *guard {
        if browser.get_version().is_err() {
            sys_debug!("[BROWSER:chrome-headless-shell] Process dead, restarting");
            *guard = None;
        }
    }

    if guard.is_none() {
        let binary = find_binary().ok_or_else(|| {
            "chrome-headless-shell binary not found. Set CHROME_HEADLESS_SHELL_PATH env var or install it.".to_string()
        })?;

        eprintln!("[BROWSER:chrome-headless-shell] Launching from {}", binary.display());
        let launch_options = LaunchOptions {
            headless: true,
            sandbox: false,
            path: Some(binary),
            window_size: Some((1280, 720)),
            idle_browser_timeout: Duration::from_secs(120),
            args: vec![
                OsStr::new("--disable-blink-features=AutomationControlled"),
            ],
            ..LaunchOptions::default()
        };

        let browser = Browser::new(launch_options)
            .map_err(|e| format!("Failed to launch chrome-headless-shell: {e}"))?;

        *guard = Some(browser);
        eprintln!("[BROWSER:chrome-headless-shell] Launched successfully");
    }

    LAST_USED.store(now, Ordering::Relaxed);
    Ok(())
}

fn new_tab() -> Result<Arc<Tab>, String> {
    let guard = HEADLESS_SHELL_BROWSER.lock().unwrap_or_else(|e| e.into_inner());
    let browser = guard.as_ref().ok_or("chrome-headless-shell not initialized")?;
    browser.new_tab().map_err(|e| format!("Failed to create tab: {e}"))
}

/// Fetch page as plain text.
pub fn fetch_text(url: &str, max_chars: usize) -> Result<String, String> {
    let url_owned = url.to_string();
    let (tx, rx) = std::sync::mpsc::channel();

    std::thread::spawn(move || {
        let result = fetch_text_inner(&url_owned, max_chars);
        let _ = tx.send(result);
    });

    match rx.recv_timeout(FETCH_HARD_TIMEOUT) {
        Ok(result) => result,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            shutdown();
            Err(format!("chrome-headless-shell fetch timed out after {}s", FETCH_HARD_TIMEOUT.as_secs()))
        }
        Err(e) => Err(format!("chrome-headless-shell fetch thread error: {e}")),
    }
}

fn fetch_text_inner(url: &str, max_chars: usize) -> Result<String, String> {
    get_or_init()?;
    let tab = new_tab()?;
    tab.set_default_timeout(NAV_TIMEOUT);

    if let Err(e) = tab.enable_stealth_mode() {
        eprintln!("[BROWSER:chrome-headless-shell] Stealth mode failed (non-fatal): {e}");
    }

    tab.navigate_to(url).map_err(|e| format!("Navigation failed: {e}"))?;
    tab.wait_until_navigated().map_err(|e| format!("Navigation wait failed: {e}"))?;
    std::thread::sleep(Duration::from_millis(500));

    let html = tab.get_content().map_err(|e| format!("Failed to get content: {e}"))?;
    let _ = tab.close(true);

    let text = html2text::from_read(html.as_bytes(), 120);
    if text.len() > max_chars {
        let mut t = max_chars;
        while t > 0 && !text.is_char_boundary(t) { t -= 1; }
        Ok(format!("{}...\n[Truncated: first {} of {} chars]", &text[..t], t, text.len()))
    } else {
        Ok(text)
    }
}

/// Fetch raw HTML.
pub fn fetch_html(url: &str) -> Result<String, String> {
    let url_owned = url.to_string();
    let (tx, rx) = std::sync::mpsc::channel();

    std::thread::spawn(move || {
        let result = fetch_html_inner(&url_owned);
        let _ = tx.send(result);
    });

    match rx.recv_timeout(FETCH_HARD_TIMEOUT) {
        Ok(result) => result,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            shutdown();
            Err(format!("chrome-headless-shell HTML fetch timed out after {}s", FETCH_HARD_TIMEOUT.as_secs()))
        }
        Err(e) => Err(format!("chrome-headless-shell HTML fetch thread error: {e}")),
    }
}

fn fetch_html_inner(url: &str) -> Result<String, String> {
    get_or_init()?;
    let tab = new_tab()?;
    tab.set_default_timeout(NAV_TIMEOUT);

    if let Err(e) = tab.enable_stealth_mode() {
        eprintln!("[BROWSER:chrome-headless-shell] Stealth mode failed (non-fatal): {e}");
    }

    tab.navigate_to(url).map_err(|e| format!("Navigation failed: {e}"))?;
    tab.wait_until_navigated().map_err(|e| format!("Navigation wait failed: {e}"))?;
    std::thread::sleep(Duration::from_millis(500));

    let html = tab.get_content().map_err(|e| format!("Failed to get content: {e}"))?;
    let _ = tab.close(true);
    Ok(html)
}

/// Shut down the browser to free memory.
pub fn shutdown() {
    if let Ok(mut guard) = HEADLESS_SHELL_BROWSER.lock() {
        if guard.is_some() {
            sys_debug!("[BROWSER:chrome-headless-shell] Shutting down");
            *guard = None;
        }
    }
}
