//! Headless Chrome browser singleton for JS-rendered web page fetching.
//!
//! Provides a headless Chrome instance for fetching web pages with full JavaScript
//! rendering. Falls back gracefully if Chrome is not installed.

use headless_chrome::{Browser, LaunchOptions, Tab};
use std::ffi::OsStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::sys_debug;

/// How long the browser can sit idle before we kill it to free memory (~150MB).
const BROWSER_IDLE_TIMEOUT_SECS: u64 = 300; // 5 minutes

/// Default timeout for page navigation.
const NAV_TIMEOUT: Duration = Duration::from_secs(15);

/// Hard timeout for the entire chrome_web_fetch operation.
/// Covers browser launch + tab creation + navigation + content extraction.
/// If anything hangs (DNS, Chrome startup, unresponsive DevTools), this kills it.
const FETCH_HARD_TIMEOUT: Duration = Duration::from_secs(25);

lazy_static::lazy_static! {
    static ref CHROME_BROWSER: Mutex<Option<Browser>> = Mutex::new(None);
}

/// Epoch-seconds timestamp of last browser use (for idle eviction).
static LAST_USED: AtomicU64 = AtomicU64::new(0);

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Get or initialize the browser singleton.
/// Drops and re-creates if idle too long or if the Chrome process died.
fn get_or_init_browser() -> Result<(), String> {
    let mut guard = CHROME_BROWSER
        .lock()
        .unwrap_or_else(|e| e.into_inner()); // recover from poisoned mutex

    let now = now_epoch_secs();
    let last = LAST_USED.load(Ordering::Relaxed);

    // Evict if idle too long
    if guard.is_some() && last > 0 && now.saturating_sub(last) > BROWSER_IDLE_TIMEOUT_SECS {
        sys_debug!("[BROWSER] Idle timeout ({}s), shutting down Chrome", now - last);
        *guard = None;
    }

    // Health check: verify the browser process is still alive
    if let Some(ref browser) = *guard {
        if browser.get_version().is_err() {
            sys_debug!("[BROWSER] Chrome process appears dead, restarting...");
            *guard = None;
        }
    }

    if guard.is_none() {
        eprintln!("[BROWSER] Launching headless Chrome...");
        let launch_options = LaunchOptions {
            headless: false, // We use --headless=new via args instead (less detectable)
            sandbox: false,
            window_size: Some((1280, 720)),
            idle_browser_timeout: Duration::from_secs(120),
            args: vec![
                // Use Chrome's new headless mode (virtually undetectable, Chrome 112+)
                OsStr::new("--headless=new"),
                // Prevent navigator.webdriver=true detection
                OsStr::new("--disable-blink-features=AutomationControlled"),
            ],
            ..LaunchOptions::default()
        };

        let browser = Browser::new(launch_options)
            .map_err(|e| {
                eprintln!("[BROWSER] Failed to launch Chrome: {e}");
                format!("Failed to launch Chrome: {e}")
            })?;

        *guard = Some(browser);
        eprintln!("[BROWSER] Chrome launched successfully");
    }

    LAST_USED.store(now, Ordering::Relaxed);
    Ok(())
}

/// Create a new tab from the singleton browser.
/// Lock is held only during tab creation, then released.
fn new_tab() -> Result<Arc<Tab>, String> {
    let guard = CHROME_BROWSER
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let browser = guard.as_ref().ok_or("Browser not initialized")?;
    browser
        .new_tab()
        .map_err(|e| format!("Failed to create tab: {e}"))
}

/// Fetch a web page using headless Chrome (JS-rendered content).
/// Wrapped in a hard timeout to prevent indefinite blocking when Chrome hangs.
pub fn chrome_web_fetch(url: &str, max_chars: usize) -> Result<String, String> {
    let url_owned = url.to_string();
    let (tx, rx) = std::sync::mpsc::channel();

    std::thread::spawn(move || {
        let result = chrome_web_fetch_inner(&url_owned, max_chars);
        let _ = tx.send(result);
    });

    match rx.recv_timeout(FETCH_HARD_TIMEOUT) {
        Ok(result) => result,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            eprintln!("[BROWSER] chrome_web_fetch hard timeout ({}s) for URL, killing browser",
                FETCH_HARD_TIMEOUT.as_secs());
            // Kill the browser so the spawned thread unblocks on its next CDP call
            shutdown_browser();
            Err(format!("Chrome fetch timed out after {}s", FETCH_HARD_TIMEOUT.as_secs()))
        }
        Err(e) => Err(format!("Chrome fetch thread error: {e}")),
    }
}

fn chrome_web_fetch_inner(url: &str, max_chars: usize) -> Result<String, String> {
    get_or_init_browser()?;
    let tab = new_tab()?;

    let result = do_fetch(&tab, url, max_chars);

    let _ = tab.close(true);

    result
}

fn do_fetch(tab: &Tab, url: &str, max_chars: usize) -> Result<String, String> {
    tab.set_default_timeout(NAV_TIMEOUT);
    tab.navigate_to(url)
        .map_err(|e| format!("Navigation failed: {e}"))?;
    tab.wait_until_navigated()
        .map_err(|e| format!("Navigation wait failed: {e}"))?;

    // Brief pause for JS rendering
    std::thread::sleep(Duration::from_millis(500));

    let html = tab
        .get_content()
        .map_err(|e| format!("Failed to get page content: {e}"))?;

    // Convert HTML to plain text
    let text = html2text::from_read(html.as_bytes(), 120);

    if text.len() > max_chars {
        Ok(format!(
            "{}...\n[Truncated: first {} of {} chars]",
            &text[..max_chars],
            max_chars,
            text.len()
        ))
    } else {
        Ok(text)
    }
}

/// Explicitly shut down the browser to free memory.
#[allow(dead_code)]
pub fn shutdown_browser() {
    if let Ok(mut guard) = CHROME_BROWSER.lock() {
        if guard.is_some() {
            sys_debug!("[BROWSER] Shutting down Chrome");
            *guard = None; // Drop triggers Chrome process kill
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Integration test: verify Chrome can fetch a web page.
    /// Run with: cargo test --bin llama_chat_web chrome_fetch_integration -- --ignored --nocapture
    #[test]
    #[ignore] // requires Chrome installed, network access
    fn chrome_fetch_integration() {
        eprintln!("--- Starting Chrome fetch integration test ---");
        let result = chrome_web_fetch("https://example.com", 5000);
        match &result {
            Ok(content) => {
                eprintln!("SUCCESS: Got {} chars of content", content.len());
                eprintln!("First 300 chars:\n{}", &content[..content.len().min(300)]);
                assert!(content.contains("Example"), "Expected 'Example' in page content");
            }
            Err(e) => {
                eprintln!("FAILED: {e}");
                panic!("Chrome fetch failed: {e}");
            }
        }
        shutdown_browser();
    }
}
