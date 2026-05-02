//! Browser panel Tauri commands — native WebView child window controls.

use tauri::{AppHandle, Manager};
use tauri::{LogicalPosition, LogicalSize, WebviewUrl};
use tauri::webview::WebviewBuilder;

// ─── Native browser panel (child WebView, no iframe restrictions) ───
//
// Opens a real native webview as a CHILD of the main window, positioned to
// look embedded inside the React UI. Unlike `<iframe>`, this webview is a
// top-level browser process — sites with X-Frame-Options (Google, Twitter,
// banks) load normally. Requires the `unstable` Tauri feature flag.

const BROWSER_PANEL_LABEL: &str = "browser-panel";

fn parse_url(s: &str) -> Result<tauri::Url, String> {
    s.parse::<tauri::Url>().map_err(|e| format!("Invalid URL: {e}"))
}

#[tauri::command]
pub async fn browser_panel_open(
    app: AppHandle,
    url: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Result<(), String> {
    let main_window = app
        .get_window("main")
        .ok_or("Main window not found")?;
    // Close any existing panel first so we don't leak webviews
    if let Some(existing) = app.webviews().get(BROWSER_PANEL_LABEL) {
        let _ = existing.close();
        tokio::time::sleep(std::time::Duration::from_millis(80)).await;
    }
    let parsed = parse_url(&url)?;
    // Persistent data directory for cookies/sessions across app restarts
    let data_dir = app.path().app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join("browser_data");
    let builder = WebviewBuilder::new(BROWSER_PANEL_LABEL, WebviewUrl::External(parsed))
        .data_directory(data_dir)
        .zoom_hotkeys_enabled(true);
    main_window
        .add_child(
            builder,
            LogicalPosition::new(x, y),
            LogicalSize::new(width.max(50.0), height.max(50.0)),
        )
        .map_err(|e| format!("Failed to attach webview: {e}"))?;
    Ok(())
}

#[tauri::command]
pub async fn browser_panel_navigate(app: AppHandle, url: String) -> Result<(), String> {
    let webview = app
        .webviews()
        .get(BROWSER_PANEL_LABEL)
        .cloned()
        .ok_or("Browser panel not open")?;
    let parsed = parse_url(&url)?;
    webview
        .navigate(parsed)
        .map_err(|e| format!("Navigate failed: {e}"))?;
    Ok(())
}

#[tauri::command]
pub async fn browser_panel_get_info(app: AppHandle) -> Result<serde_json::Value, String> {
    let webview = app.webviews().get(BROWSER_PANEL_LABEL).cloned()
        .ok_or("Browser panel not open")?;
    let url = webview.url().map(|u| u.to_string()).unwrap_or_default();
    // Get title via eval — fire-and-forget eval doesn't return, so we use
    // a simple heuristic: return the URL-based title for now.
    // The frontend will poll this periodically.
    Ok(serde_json::json!({ "url": url }))
}

#[tauri::command]
pub async fn browser_panel_zoom(app: AppHandle, delta: f64) -> Result<f64, String> {
    let webview = app.webviews().get(BROWSER_PANEL_LABEL).cloned()
        .ok_or("Browser panel not open")?;
    // Get current zoom, apply delta
    // WebView2 default zoom is 1.0, range 0.25-5.0
    // We'll use eval to read/set since Tauri's set_zoom is available
    let current = webview.url().map(|_| 1.0_f64).unwrap_or(1.0); // placeholder
    let new_zoom = (current + delta).clamp(0.25, 5.0);
    webview.set_zoom(new_zoom).map_err(|e| format!("Zoom failed: {e}"))?;
    Ok(new_zoom)
}

#[tauri::command]
pub async fn browser_panel_set_zoom(app: AppHandle, zoom: f64) -> Result<(), String> {
    let webview = app.webviews().get(BROWSER_PANEL_LABEL).cloned()
        .ok_or("Browser panel not open")?;
    webview.set_zoom(zoom.clamp(0.25, 5.0)).map_err(|e| format!("Zoom failed: {e}"))?;
    Ok(())
}

#[tauri::command]
pub async fn browser_panel_eval_js(app: AppHandle, js: String) -> Result<(), String> {
    let webview = app.webviews().get(BROWSER_PANEL_LABEL).cloned()
        .ok_or("Browser panel not open")?;
    webview.eval(&js).map_err(|e| format!("Eval failed: {e}"))?;
    Ok(())
}

#[tauri::command]
pub async fn browser_panel_reload(app: AppHandle) -> Result<(), String> {
    let webview = app.webviews().get(BROWSER_PANEL_LABEL).cloned()
        .ok_or("Browser panel not open")?;
    webview.eval("window.location.reload()").map_err(|e| format!("Reload failed: {e}"))?;
    Ok(())
}

#[tauri::command]
pub async fn browser_panel_go_back(app: AppHandle) -> Result<(), String> {
    let webview = app.webviews().get(BROWSER_PANEL_LABEL).cloned()
        .ok_or("Browser panel not open")?;
    webview.eval("window.history.back()").map_err(|e| format!("Back failed: {e}"))?;
    Ok(())
}

#[tauri::command]
pub async fn browser_panel_go_forward(app: AppHandle) -> Result<(), String> {
    let webview = app.webviews().get(BROWSER_PANEL_LABEL).cloned()
        .ok_or("Browser panel not open")?;
    webview.eval("window.history.forward()").map_err(|e| format!("Forward failed: {e}"))?;
    Ok(())
}

#[tauri::command]
pub async fn browser_panel_resize(
    app: AppHandle,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Result<(), String> {
    let webview = app
        .webviews()
        .get(BROWSER_PANEL_LABEL)
        .cloned()
        .ok_or("Browser panel not open")?;
    webview
        .set_position(LogicalPosition::new(x, y))
        .map_err(|e| format!("set_position failed: {e}"))?;
    webview
        .set_size(LogicalSize::new(width.max(50.0), height.max(50.0)))
        .map_err(|e| format!("set_size failed: {e}"))?;
    Ok(())
}

// ─── Agent browser API ───────────────────────────────────────────────
//
// Exposes the agent's browser tools as Tauri commands for external control.
use llama_chat_tools::browser_tools::handle_browser_tool;
// These use the hidden "agent-browser" WebView (same one the LLM agent uses),
// allowing Claude Code or other external tools to navigate, read, click, etc.

#[tauri::command]
pub async fn agent_browser_navigate(url: String) -> Result<String, String> {
    let args = serde_json::json!({"url": url});
    let result = handle_browser_tool("navigate", &args);
    Ok(result.text)
}

#[tauri::command]
pub async fn agent_browser_get_text(summary: Option<String>) -> Result<String, String> {
    let args = serde_json::json!({"summary": summary.unwrap_or_else(|| "false".into())});
    let result = handle_browser_tool("get_text", &args);
    Ok(result.text)
}

#[tauri::command]
pub async fn agent_browser_get_links() -> Result<String, String> {
    let args = serde_json::json!({});
    let result = handle_browser_tool("get_links", &args);
    Ok(result.text)
}

#[tauri::command]
pub async fn agent_browser_get_html() -> Result<String, String> {
    let args = serde_json::json!({});
    let result = handle_browser_tool("get_html", &args);
    Ok(result.text)
}

#[tauri::command]
pub async fn agent_browser_click(selector: String) -> Result<String, String> {
    let args = serde_json::json!({"selector": selector});
    let result = handle_browser_tool("click", &args);
    Ok(result.text)
}

#[tauri::command]
pub async fn agent_browser_type_text(selector: String, text: String) -> Result<String, String> {
    let args = serde_json::json!({"selector": selector, "text": text});
    let result = handle_browser_tool("type", &args);
    Ok(result.text)
}

#[tauri::command]
pub async fn agent_browser_eval(js: String) -> Result<String, String> {
    let args = serde_json::json!({"js": js});
    let result = handle_browser_tool("eval", &args);
    Ok(result.text)
}

#[tauri::command]
pub async fn agent_browser_search(query: String) -> Result<String, String> {
    let args = serde_json::json!({"query": query});
    let result = handle_browser_tool("search", &args);
    Ok(result.text)
}

#[tauri::command]
pub async fn agent_browser_scroll(direction: Option<String>, amount: Option<i32>) -> Result<String, String> {
    let args = serde_json::json!({"direction": direction.unwrap_or_else(|| "down".into()), "amount": amount.unwrap_or(3)});
    let result = handle_browser_tool("scroll", &args);
    Ok(result.text)
}

#[tauri::command]
pub async fn agent_browser_query(selector: String, extract: Option<String>) -> Result<String, String> {
    let args = serde_json::json!({"selector": selector, "extract": extract});
    let result = handle_browser_tool("query", &args);
    Ok(result.text)
}

#[tauri::command]
pub async fn browser_panel_close(app: AppHandle) -> Result<(), String> {
    if let Some(webview) = app.webviews().get(BROWSER_PANEL_LABEL).cloned() {
        webview.close().map_err(|e| format!("Close failed: {e}"))?;
    }
    Ok(())
}
