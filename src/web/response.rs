// Response helper utilities for HTTP responses with CORS headers

use hyper::{Body, Response, StatusCode};

/// Create a JSON response with CORS headers
pub fn json_response(status: StatusCode, body: impl Into<Body>) -> Response<Body> {
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .header("access-control-allow-origin", "*")
        .body(body.into())
        .unwrap()
}

/// Create a successful JSON response (200 OK)
pub fn json_ok(body: impl Into<Body>) -> Response<Body> {
    json_response(StatusCode::OK, body)
}

/// Create a bad request error response (400)
pub fn bad_request(error_msg: &str) -> Response<Body> {
    let body = format!(r#"{{"error":"{}"}}"#, error_msg);
    json_response(StatusCode::BAD_REQUEST, body)
}

/// Create an internal server error response (500)
pub fn internal_error(error_msg: &str) -> Response<Body> {
    let body = format!(r#"{{"error":"{}"}}"#, error_msg);
    json_response(StatusCode::INTERNAL_SERVER_ERROR, body)
}

/// Create a not found error response (404)
pub fn not_found(error_msg: &str) -> Response<Body> {
    let body = format!(r#"{{"error":"{}"}}"#, error_msg);
    json_response(StatusCode::NOT_FOUND, body)
}

/// Create a CORS preflight response (OPTIONS)
pub fn cors_preflight() -> Response<Body> {
    Response::builder()
        .status(StatusCode::OK)
        .header("access-control-allow-origin", "*")
        .header("access-control-allow-methods", "GET, POST, PUT, DELETE, OPTIONS")
        .header("access-control-allow-headers", "content-type")
        .body(Body::empty())
        .unwrap()
}

/// Create an HTML response with CORS headers
pub fn html_response(status: StatusCode, body: impl Into<Body>) -> Response<Body> {
    Response::builder()
        .status(status)
        .header("content-type", "text/html")
        .header("access-control-allow-origin", "*")
        .body(body.into())
        .unwrap()
}

/// Create a text/plain response with CORS headers
pub fn text_response(status: StatusCode, body: impl Into<Body>) -> Response<Body> {
    Response::builder()
        .status(status)
        .header("content-type", "text/plain")
        .header("access-control-allow-origin", "*")
        .body(body.into())
        .unwrap()
}

/// Create a Server-Sent Events response with CORS headers
pub fn sse_response(body: Body) -> Response<Body> {
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-cache")
        .header("access-control-allow-origin", "*")
        .body(body)
        .unwrap()
}
