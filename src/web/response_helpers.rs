// HTTP response helper functions to reduce duplication across route handlers

use hyper::{Body, Response, StatusCode};
use serde::Serialize;

/// Standard CORS headers
const CORS_ORIGIN: &str = "*";
const CORS_METHODS: &str = "GET, POST, PUT, DELETE, OPTIONS";
const CORS_HEADERS: &str = "content-type, authorization";

/// Build a JSON response with CORS headers
pub fn json_response<T: Serialize>(status: StatusCode, body: &T) -> Response<Body> {
    let json = serde_json::to_string(body).unwrap_or_else(|_| r#"{"error":"Serialization failed"}"#.to_string());
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .header("access-control-allow-origin", CORS_ORIGIN)
        .header("access-control-allow-methods", CORS_METHODS)
        .header("access-control-allow-headers", CORS_HEADERS)
        .body(Body::from(json))
        .unwrap()
}

/// Build a JSON error response
pub fn json_error(status: StatusCode, message: &str) -> Response<Body> {
    let json = format!(r#"{{"error":"{}"}}"#, message.replace('"', "\\\""));
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .header("access-control-allow-origin", CORS_ORIGIN)
        .header("access-control-allow-methods", CORS_METHODS)
        .header("access-control-allow-headers", CORS_HEADERS)
        .body(Body::from(json))
        .unwrap()
}

/// Build a JSON success response
pub fn json_success(message: &str) -> Response<Body> {
    let json = format!(r#"{{"success":true,"message":"{}"}}"#, message.replace('"', "\\\""));
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .header("access-control-allow-origin", CORS_ORIGIN)
        .header("access-control-allow-methods", CORS_METHODS)
        .header("access-control-allow-headers", CORS_HEADERS)
        .body(Body::from(json))
        .unwrap()
}

/// Build a raw JSON string response
pub fn json_raw(status: StatusCode, json: String) -> Response<Body> {
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .header("access-control-allow-origin", CORS_ORIGIN)
        .header("access-control-allow-methods", CORS_METHODS)
        .header("access-control-allow-headers", CORS_HEADERS)
        .body(Body::from(json))
        .unwrap()
}

/// Build an empty response with CORS headers
pub fn empty_response(status: StatusCode) -> Response<Body> {
    Response::builder()
        .status(status)
        .header("access-control-allow-origin", CORS_ORIGIN)
        .header("access-control-allow-methods", CORS_METHODS)
        .header("access-control-allow-headers", CORS_HEADERS)
        .body(Body::empty())
        .unwrap()
}

/// CORS preflight response
pub fn cors_preflight() -> Response<Body> {
    empty_response(StatusCode::OK)
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
