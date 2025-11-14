// Request parsing utilities for HTTP requests

use hyper::{Body, Request, Response};
use serde::de::DeserializeOwned;

use super::response;

/// Parse request body as JSON
/// Returns the deserialized value or an error response
pub async fn parse_json<T: DeserializeOwned>(req: Request<Body>) -> Result<T, Response<Body>> {
    // Read body bytes
    let body_bytes = match hyper::body::to_bytes(req.into_body()).await {
        Ok(bytes) => bytes,
        Err(e) => {
            eprintln!("[ERROR] Failed to read request body: {}", e);
            return Err(response::bad_request("Failed to read request body"));
        }
    };

    // Parse JSON
    match serde_json::from_slice(&body_bytes) {
        Ok(value) => Ok(value),
        Err(e) => {
            eprintln!("[ERROR] JSON parsing error: {}", e);
            Err(response::bad_request("Invalid JSON format"))
        }
    }
}

/// Extract path parameter from URI path
/// Example: extract_path_param("/api/conversations/chat123.txt", "/api/conversations/") => Some("chat123.txt")
pub fn extract_path_param<'a>(path: &'a str, prefix: &str) -> Option<&'a str> {
    if path.starts_with(prefix) {
        Some(&path[prefix.len()..])
    } else {
        None
    }
}

/// Parse query string parameter
/// Example: parse_query_param("?model=foo&temp=0.7", "temp") => Some("0.7")
pub fn parse_query_param<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    for param in query.split('&') {
        if let Some((k, v)) = param.split_once('=') {
            if k == key {
                return Some(v);
            }
        }
    }
    None
}
