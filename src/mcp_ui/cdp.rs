//! Chrome DevTools Protocol (CDP) helpers and WebView2 screenshot capture.
//!
//! `cdp_call`     — raw CDP method invocation via vtable index 36
//! `cdp_click`    — element click using CDP Input.dispatchMouseEvent
//! `capture_webview_screenshot` — PNG capture via CapturePreview vtable index 30

use serde_json::{Value, json};
use tauri::{AppHandle, Manager};
use tokio::sync::oneshot;

use super::eval::eval_js_in;

/// Call a Chrome DevTools Protocol method on a webview.
/// Uses WebView2's CallDevToolsProtocolMethod COM API (vtable index 36).
#[allow(unused_variables)]
pub async fn cdp_call(
    app: &AppHandle,
    target: &str,
    method: &str,
    params: &Value,
) -> Result<String, String> {
    let (tx, rx) = oneshot::channel::<String>();

    let webviews = app.webviews();
    let webview = webviews.get(target).cloned()
        .ok_or_else(|| format!("Webview '{target}' not open"))?;

    #[cfg(windows)]
    {
        let method_str = method.to_string();
        let params_str = params.to_string();

        webview.with_webview(move |platform_wv| {
            let controller = platform_wv.controller();
            let core_wv = unsafe { controller.CoreWebView2() };

            match core_wv {
                Ok(core) => {
                    let handler = webview2_com::CallDevToolsProtocolMethodCompletedHandler::create(
                        Box::new(move |_hr, result| {
                            let _ = tx.send(result);
                            Ok(())
                        }),
                    );
                    let method_wide: Vec<u16> = method_str.encode_utf16()
                        .chain(std::iter::once(0)).collect();
                    let params_wide: Vec<u16> = params_str.encode_utf16()
                        .chain(std::iter::once(0)).collect();

                    // CallDevToolsProtocolMethod vtable index = 36
                    unsafe {
                        let this: *mut std::ffi::c_void = std::mem::transmute_copy(&core);
                        let vtable = *(this as *const *const usize);
                        type CdpFn = unsafe extern "system" fn(
                            this: *mut std::ffi::c_void,
                            method: *const u16,
                            params: *const u16,
                            handler: *mut std::ffi::c_void,
                        ) -> i32;
                        let func: CdpFn = std::mem::transmute(*vtable.add(36));
                        let handler_ptr: *mut std::ffi::c_void =
                            std::mem::transmute_copy(&handler);
                        func(this, method_wide.as_ptr(), params_wide.as_ptr(), handler_ptr);
                    }
                }
                Err(e) => {
                    let _ = tx.send(format!(
                        r#"{{"error":"CoreWebView2 unavailable: {e}"}}"#
                    ));
                }
            }
        }).map_err(|e| format!("with_webview failed: {e}"))?;
    }

    #[cfg(not(windows))]
    {
        let _ = tx.send(r#"{"error":"CDP not available on this platform"}"#.to_string());
    }

    match tokio::time::timeout(std::time::Duration::from_secs(10), rx).await {
        Ok(Ok(result)) => Ok(result),
        Ok(Err(_)) => Err("CDP channel closed".into()),
        Err(_) => Err("CDP call timed out (10s)".into()),
    }
}

/// Click an element using CDP Input.dispatchMouseEvent.
/// First finds element bounds via JS eval, then sends real mouse events via CDP.
pub async fn cdp_click(app: &AppHandle, target: &str, selector: &str) -> Result<String, String> {
    // Step 1: Find element bounds via JS
    let js = format!(
        r#"(() => {{
            const el = document.querySelector({sel});
            if (!el) return {{error: 'Element not found: ' + {sel}}};
            const rect = el.getBoundingClientRect();
            return {{
                x: Math.round(rect.left + rect.width / 2),
                y: Math.round(rect.top + rect.height / 2),
                text: (el.textContent || '').trim().slice(0, 50),
                tag: el.tagName
            }};
        }})()"#,
        sel = serde_json::to_string(selector).unwrap()
    );
    let bounds_str = eval_js_in(app, &js, target).await?;
    // eval_js_in wraps in JSON.stringify, so bounds_str is already valid JSON
    let bounds: Value = serde_json::from_str(&bounds_str)
        .map_err(|e| format!("Failed to parse bounds: {e} — raw: {bounds_str}"))?;

    if let Some(err) = bounds.get("error").and_then(|e| e.as_str()) {
        return Err(err.to_string());
    }

    let x = bounds["x"].as_f64().ok_or("Missing x coordinate")?;
    let y = bounds["y"].as_f64().ok_or("Missing y coordinate")?;
    let text = bounds["text"].as_str().unwrap_or("");
    let tag = bounds["tag"].as_str().unwrap_or("?");

    // Step 2: Send CDP mouse events (mousePressed + mouseReleased = full click)
    let press_params = json!({
        "type": "mousePressed",
        "x": x,
        "y": y,
        "button": "left",
        "clickCount": 1
    });
    cdp_call(app, target, "Input.dispatchMouseEvent", &press_params).await?;

    // Small delay between press and release
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let release_params = json!({
        "type": "mouseReleased",
        "x": x,
        "y": y,
        "button": "left",
        "clickCount": 1
    });
    cdp_call(app, target, "Input.dispatchMouseEvent", &release_params).await?;

    Ok(format!("CDP clicked {tag}: {text} at ({x}, {y})"))
}

/// Capture a screenshot of a webview and save as PNG.
/// Uses WebView2's CapturePreview COM API via raw vtable (index 30).
#[allow(unused_variables)]
pub async fn capture_webview_screenshot(app: &AppHandle, target: &str) -> Result<String, String> {
    let webviews = app.webviews();
    let webview = webviews.get(target).cloned()
        .ok_or_else(|| format!("Webview '{target}' not open. Use browser_navigate first."))?;

    let screenshot_dir = app.path().app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join("screenshots");
    let _ = std::fs::create_dir_all(&screenshot_dir);
    let filename = format!("browser_{}.png", chrono::Utc::now().format("%Y%m%d_%H%M%S"));
    let filepath = screenshot_dir.join(&filename);
    let filepath_str = filepath.to_string_lossy().to_string();

    #[cfg(windows)]
    {
        let (tx, rx) = oneshot::channel::<Result<Vec<u8>, String>>();
        let filepath_clone = filepath.clone();
        let _ = filepath_clone; // suppress unused warning

        webview.with_webview(move |platform_wv| {
            let controller = platform_wv.controller();
            let core_wv = unsafe { controller.CoreWebView2() };

            match core_wv {
                Ok(core) => {
                    // Create an IStream in memory to receive the PNG
                    let stream = unsafe {
                        windows::Win32::System::Com::StructuredStorage::CreateStreamOnHGlobal(
                            windows::Win32::Foundation::HGLOBAL::default(),
                            true,
                        )
                    };
                    match stream {
                        Ok(stream) => {
                            let stream_clone = stream.clone();
                            let handler = webview2_com::CapturePreviewCompletedHandler::create(
                                Box::new(move |hr| {
                                    if hr.is_err() {
                                        let _ = tx.send(Err(format!(
                                            "CapturePreview failed: {hr:?}"
                                        )));
                                        return Ok(());
                                    }
                                    // Read PNG bytes from stream
                                    use windows::Win32::System::Com::STREAM_SEEK_SET;
                                    unsafe {
                                        let _ = stream_clone.Seek(0, STREAM_SEEK_SET, None);
                                        let mut buf = vec![0u8; 16 * 1024 * 1024]; // 16MB max
                                        let mut bytes_read = 0u32;
                                        let _ = stream_clone.Read(
                                            buf.as_mut_ptr() as *mut _,
                                            buf.len() as u32,
                                            Some(&mut bytes_read),
                                        );
                                        buf.truncate(bytes_read as usize);
                                        let _ = tx.send(Ok(buf));
                                    }
                                    Ok(())
                                }),
                            );
                            // CapturePreview vtable index:
                            // ICoreWebView2 methods after IUnknown(3):
                            // Index 30 = CapturePreview
                            // (COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT, IStream, handler)
                            unsafe {
                                let this: *mut std::ffi::c_void =
                                    std::mem::transmute_copy(&core);
                                let vtable = *(this as *const *const usize);
                                type CapturePreviewFn = unsafe extern "system" fn(
                                    this: *mut std::ffi::c_void,
                                    image_format: i32, // 0 = PNG
                                    stream: *mut std::ffi::c_void,
                                    handler: *mut std::ffi::c_void,
                                ) -> i32;
                                let func: CapturePreviewFn =
                                    std::mem::transmute(*vtable.add(30));
                                let stream_ptr: *mut std::ffi::c_void =
                                    std::mem::transmute_copy(&stream);
                                let handler_ptr: *mut std::ffi::c_void =
                                    std::mem::transmute_copy(&handler);
                                func(this, 0, stream_ptr, handler_ptr); // 0 = PNG format
                            }
                        }
                        Err(e) => {
                            let _ = tx.send(Err(format!(
                                "CreateStreamOnHGlobal failed: {e}"
                            )));
                        }
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(format!("CoreWebView2 unavailable: {e}")));
                }
            }
        }).map_err(|e| format!("with_webview failed: {e}"))?;

        match tokio::time::timeout(std::time::Duration::from_secs(10), rx).await {
            Ok(Ok(Ok(png_bytes))) => {
                if png_bytes.is_empty() {
                    return Err("Screenshot capture returned empty data".into());
                }
                std::fs::write(&filepath, &png_bytes)
                    .map_err(|e| format!("Failed to save screenshot: {e}"))?;
                Ok(filepath_str)
            }
            Ok(Ok(Err(e))) => Err(e),
            Ok(Err(_)) => Err("Screenshot channel closed".into()),
            Err(_) => Err("Screenshot timed out (10s)".into()),
        }
    }

    #[cfg(not(windows))]
    Err("Screenshot not available on this platform".into())
}
