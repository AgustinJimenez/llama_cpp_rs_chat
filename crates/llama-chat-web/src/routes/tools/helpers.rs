use hyper::{Body, Request, Response, StatusCode};
use std::convert::Infallible;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use tokio::task::spawn_blocking;
use tokio::time::{timeout, Duration};

use crate::request_parsing::get_query_param;
use crate::response_helpers::{json_error, json_raw};

pub(super) const FETCH_TIMEOUT_SECS: u64 = 15;
const MAX_RESPONSE_BYTES: usize = 100_000;
pub(super) const MAX_TEXT_CHARS: usize = 10_000;

pub(super) async fn canonicalize_allowed(path: &str) -> Result<PathBuf, String> {
    const ROOTS: [&str; 2] = ["/app", "/app/models"];
    let input = path.to_string();
    let canonical = spawn_blocking(move || std::fs::canonicalize(&input))
        .await
        .map_err(|e| format!("Failed to resolve path: {e}"))?
        .map_err(|e| format!("Failed to resolve path: {e}"))?;

    for root in ROOTS {
        let root_path = Path::new(root);
        if canonical.starts_with(root_path) {
            return Ok(canonical);
        }
    }
    Err("Path not allowed".to_string())
}

pub fn fetch_url_as_text(url: &str, max_chars: usize) -> serde_json::Value {
    sys_debug!("[WEB_FETCH] Fetching URL: {}", url);
    let agent = ureq::AgentBuilder::new()
        .timeout_read(std::time::Duration::from_secs(FETCH_TIMEOUT_SECS))
        .timeout_connect(std::time::Duration::from_secs(10))
        .user_agent("Mozilla/5.0 (compatible; LlamaChat/1.0)")
        .build();

    let response = match agent.get(url).call() {
        Ok(r) => r,
        Err(ureq::Error::Status(code, resp)) => {
            let body = resp.into_string().unwrap_or_default();
            let mut preview_end = body.len().min(500);
            while preview_end > 0 && !body.is_char_boundary(preview_end) {
                preview_end -= 1;
            }
            return serde_json::json!({
                "success": false,
                "error": format!("HTTP {} for URL '{}'", code, url),
                "url": url,
                "status_code": code,
                "body_preview": &body[..preview_end]
            });
        }
        Err(e) => {
            return serde_json::json!({
                "success": false,
                "error": format!("Failed to fetch URL '{}': {}", url, e),
                "url": url
            });
        }
    };

    let status = response.status();
    let content_type = response.header("content-type").unwrap_or("").to_string();
    let mut body_buf = Vec::with_capacity(MAX_RESPONSE_BYTES);
    let mut reader = response.into_reader().take(MAX_RESPONSE_BYTES as u64);
    if let Err(e) = reader.read_to_end(&mut body_buf) {
        return serde_json::json!({
            "success": false,
            "error": format!("Failed to read response body: {}", e),
            "url": url
        });
    }

    let body_str = String::from_utf8_lossy(&body_buf).to_string();
    let text = if content_type.contains("text/html") || body_str.trim_start().starts_with('<') {
        html2text::from_read(body_str.as_bytes(), 120)
    } else {
        body_str
    };

    let truncated = if text.len() > max_chars {
        let mut end = max_chars;
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        format!(
            "{}...\n[TRUNCATED - showing first {} of {} chars]",
            &text[..end],
            end,
            text.len()
        )
    } else {
        text.clone()
    };

    serde_json::json!({
        "success": true,
        "result": truncated,
        "url": url,
        "status_code": status,
        "content_length": text.len()
    })
}

pub async fn handle_get_web_fetch(req: Request<Body>) -> Result<Response<Body>, Infallible> {
    let url = match get_query_param(req.uri(), "url") {
        Some(u) if !u.is_empty() => u,
        _ => return Ok(json_error(StatusCode::BAD_REQUEST, "Missing 'url' query parameter")),
    };
    let max_chars = get_query_param(req.uri(), "max_length")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(MAX_TEXT_CHARS);

    let result = match timeout(
        Duration::from_secs(FETCH_TIMEOUT_SECS + 5),
        spawn_blocking(move || fetch_url_as_text(&url, max_chars)),
    )
    .await
    {
        Ok(Ok(result)) => result,
        Ok(Err(e)) => serde_json::json!({
            "success": false,
            "error": format!("Web fetch task failed: {}", e)
        }),
        Err(_) => serde_json::json!({
            "success": false,
            "error": format!("Web fetch timed out after {}s", FETCH_TIMEOUT_SECS)
        }),
    };

    Ok(json_raw(StatusCode::OK, result.to_string()))
}

pub async fn handle_post_extract_text(req: Request<Body>) -> Result<Response<Body>, Infallible> {
    let filename = get_query_param(req.uri(), "filename").unwrap_or_default();
    if filename.is_empty() {
        return Ok(json_error(
            StatusCode::BAD_REQUEST,
            "Missing 'filename' query parameter",
        ));
    }

    let body_bytes = match hyper::body::to_bytes(req.into_body()).await {
        Ok(b) => b,
        Err(e) => {
            return Ok(json_error(
                StatusCode::BAD_REQUEST,
                &format!("Failed to read body: {e}"),
            ));
        }
    };
    if body_bytes.is_empty() {
        return Ok(json_error(StatusCode::BAD_REQUEST, "Empty file body"));
    }

    const MAX_EXTRACT_CHARS: usize = 100_000;
    let fname_lower = filename.to_ascii_lowercase();
    let bytes = body_bytes.to_vec();
    let result = spawn_blocking(move || {
        use llama_chat_tools::{
            extract_csv_structured, extract_docx_text, extract_eml_text, extract_epub_text,
            extract_odt_text, extract_pdf_text, extract_pptx_text, extract_rtf_text,
            extract_xlsx_text, extract_zip_listing, read_with_encoding_detection,
            truncate_text_content,
        };

        if fname_lower.ends_with(".pdf") {
            extract_pdf_text(&bytes, MAX_EXTRACT_CHARS)
        } else if fname_lower.ends_with(".docx") {
            extract_docx_text(&bytes, MAX_EXTRACT_CHARS)
        } else if fname_lower.ends_with(".pptx") {
            extract_pptx_text(&bytes, MAX_EXTRACT_CHARS)
        } else if fname_lower.ends_with(".xlsx")
            || fname_lower.ends_with(".xls")
            || fname_lower.ends_with(".xlsm")
        {
            extract_xlsx_text(&bytes, MAX_EXTRACT_CHARS)
        } else if fname_lower.ends_with(".epub") {
            extract_epub_text(&bytes, MAX_EXTRACT_CHARS)
        } else if fname_lower.ends_with(".odt") {
            extract_odt_text(&bytes, MAX_EXTRACT_CHARS)
        } else if fname_lower.ends_with(".rtf") {
            extract_rtf_text(&bytes, MAX_EXTRACT_CHARS)
        } else if fname_lower.ends_with(".zip") {
            extract_zip_listing(&bytes, MAX_EXTRACT_CHARS)
        } else if fname_lower.ends_with(".csv") {
            extract_csv_structured(&bytes, MAX_EXTRACT_CHARS)
        } else if fname_lower.ends_with(".eml") {
            extract_eml_text(&bytes, MAX_EXTRACT_CHARS)
        } else {
            match String::from_utf8(bytes.clone()) {
                Ok(text) => truncate_text_content(&text, MAX_EXTRACT_CHARS),
                Err(_) => read_with_encoding_detection(&bytes, MAX_EXTRACT_CHARS),
            }
        }
    })
    .await;

    match result {
        Ok(text) => {
            let json = serde_json::json!({
                "success": true,
                "filename": filename,
                "text": text,
                "chars": text.len(),
            });
            Ok(json_raw(StatusCode::OK, json.to_string()))
        }
        Err(e) => Ok(json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("Extraction failed: {e}"),
        )),
    }
}
