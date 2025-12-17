// WebSocket utility functions for route handlers
//
// This module provides shared WebSocket helpers to avoid duplication
// across routes/chat.rs and other WebSocket-related code.

use base64::{engine::general_purpose, Engine as _};
use hyper::{Body, Request, Response, StatusCode};
use sha1::{Digest, Sha1};

/// Calculate the WebSocket accept key per RFC 6455
///
/// The accept key is a SHA1 hash of the client's key concatenated with
/// the WebSocket GUID, then base64 encoded.
pub fn calculate_websocket_accept_key(key: &str) -> String {
    const WEBSOCKET_GUID: &[u8] = b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

    let mut hasher = Sha1::new();
    hasher.update(key.as_bytes());
    hasher.update(WEBSOCKET_GUID);
    let hash = hasher.finalize();
    general_purpose::STANDARD.encode(hash)
}

/// Check if a request wants to upgrade to WebSocket
pub fn is_websocket_upgrade(req: &Request<Body>) -> bool {
    req.headers()
        .get("upgrade")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.eq_ignore_ascii_case("websocket"))
        .unwrap_or(false)
}

/// Extract the WebSocket key from request headers
pub fn get_websocket_key(req: &Request<Body>) -> Option<String> {
    req.headers()
        .get("sec-websocket-key")
        .and_then(|k| k.to_str().ok())
        .map(|s| s.to_string())
}

/// Build a 101 Switching Protocols response for WebSocket upgrade
pub fn build_websocket_upgrade_response(accept_key: &str) -> Response<Body> {
    Response::builder()
        .status(StatusCode::SWITCHING_PROTOCOLS)
        .header("upgrade", "websocket")
        .header("connection", "upgrade")
        .header("sec-websocket-accept", accept_key)
        .body(Body::empty())
        .unwrap()
}

/// Build a JSON error response with CORS headers
pub fn build_json_error_response(status: StatusCode, message: &str) -> Response<Body> {
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .header("access-control-allow-origin", "*")
        .body(Body::from(format!(r#"{{"error":"{}"}}"#, message)))
        .unwrap()
}

/// Build a successful JSON response with CORS headers
/// Build a JSON response (non-WebSocket)
/// TODO: Use for HTTP fallback when WebSocket unavailable
#[allow(dead_code)]
pub fn build_json_response(json: &str) -> Response<Body> {
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .header("access-control-allow-origin", "*")
        .header("access-control-allow-methods", "GET, POST, OPTIONS")
        .header("access-control-allow-headers", "content-type")
        .body(Body::from(json.to_string()))
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_websocket_accept_key() {
        // Test vector from RFC 6455
        let key = "dGhlIHNhbXBsZSBub25jZQ==";
        let expected = "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=";
        assert_eq!(calculate_websocket_accept_key(key), expected);
    }

    #[test]
    fn test_build_json_error_response() {
        let response = build_json_error_response(StatusCode::BAD_REQUEST, "Test error");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "application/json"
        );
    }

    #[test]
    fn test_build_json_response() {
        let response = build_json_response(r#"{"test": "value"}"#);
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get("access-control-allow-origin")
                .unwrap(),
            "*"
        );
    }
}
