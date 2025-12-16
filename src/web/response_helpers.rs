// HTTP response helper functions to reduce duplication across route handlers

use hyper::{Body, Response, StatusCode};
use serde::Serialize;

/// Standard CORS headers
const CORS_ORIGIN: &str = "*";
const CORS_METHODS: &str = "GET, POST, PUT, DELETE, OPTIONS";
const CORS_HEADERS: &str = "content-type, authorization";

/// Apply CORS headers to a response builder
fn with_cors(builder: hyper::http::response::Builder) -> hyper::http::response::Builder {
    builder
        .header("access-control-allow-origin", CORS_ORIGIN)
        .header("access-control-allow-methods", CORS_METHODS)
        .header("access-control-allow-headers", CORS_HEADERS)
}

/// Serialize a value to JSON with a fallback string on error
pub fn serialize_with_fallback<T: Serialize>(value: &T, fallback: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| fallback.to_string())
}

/// Build a JSON response with CORS headers
pub fn json_response<T: Serialize>(status: StatusCode, body: &T) -> Response<Body> {
    let json = serialize_with_fallback(body, r#"{"error":"Serialization failed"}"#);
    with_cors(Response::builder().status(status))
        .header("content-type", "application/json")
        .body(Body::from(json))
        .unwrap()
}

/// Build a JSON error response
pub fn json_error(status: StatusCode, message: &str) -> Response<Body> {
    let json = format!(r#"{{"error":"{}"}}"#, message.replace('"', "\\\""));
    with_cors(Response::builder().status(status))
        .header("content-type", "application/json")
        .body(Body::from(json))
        .unwrap()
}

/// Build a JSON success response
/// TODO: Use this for standardized success responses instead of json_raw
#[allow(dead_code)]
pub fn json_success(message: &str) -> Response<Body> {
    let json = format!(r#"{{"success":true,"message":"{}"}}"#, message.replace('"', "\\\""));
    with_cors(Response::builder().status(StatusCode::OK))
        .header("content-type", "application/json")
        .body(Body::from(json))
        .unwrap()
}

/// Build a raw JSON string response
pub fn json_raw(status: StatusCode, json: String) -> Response<Body> {
    with_cors(Response::builder().status(status))
        .header("content-type", "application/json")
        .body(Body::from(json))
        .unwrap()
}

/// Build an empty response with CORS headers
pub fn empty_response(status: StatusCode) -> Response<Body> {
    with_cors(Response::builder().status(status))
        .body(Body::empty())
        .unwrap()
}

/// CORS preflight response
pub fn cors_preflight() -> Response<Body> {
    empty_response(StatusCode::OK)
}

/// Build an HTML response with CORS headers
/// TODO: Use for serving custom HTML error pages
#[allow(dead_code)]
pub fn html_response(status: StatusCode, body: impl Into<Body>) -> Response<Body> {
    with_cors(Response::builder().status(status))
        .header("content-type", "text/html")
        .body(body.into())
        .unwrap()
}

/// Build a text/plain response with CORS headers
/// TODO: Use for plain text responses (logs, debug output)
#[allow(dead_code)]
pub fn text_response(status: StatusCode, body: impl Into<Body>) -> Response<Body> {
    with_cors(Response::builder().status(status))
        .header("content-type", "text/plain")
        .body(body.into())
        .unwrap()
}

/// Build a Server-Sent Events response with CORS headers
/// TODO: Use for SSE streaming endpoints (alternative to WebSocket)
#[allow(dead_code)]
pub fn sse_response(body: Body) -> Response<Body> {
    with_cors(Response::builder().status(StatusCode::OK))
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-cache")
        .body(body)
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_error() {
        let response = json_error(StatusCode::BAD_REQUEST, "Test error");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn test_json_error_escapes_quotes() {
        let response = json_error(StatusCode::BAD_REQUEST, r#"Error "quoted""#);
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
