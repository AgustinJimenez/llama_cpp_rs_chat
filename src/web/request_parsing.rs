// Request parsing utilities for HTTP handlers

use hyper::{Body, Response, StatusCode, Uri};
use serde::de::DeserializeOwned;

// Import logging macros
use crate::{sys_debug, sys_error};

/// Parse JSON request body into a typed structure.
///
/// Returns the deserialized value on success, or an error Response on failure.
/// The error Response includes proper CORS headers and error message in JSON format.
///
/// # Example
/// ```
/// let chat_request: ChatRequest = match parse_json_body(req.into_body()).await {
///     Ok(req) => req,
///     Err(error_response) => return Ok(error_response),
/// };
/// ```
pub async fn parse_json_body<T: DeserializeOwned>(body: Body) -> Result<T, Response<Body>> {
    // Read body bytes
    let body_bytes = match hyper::body::to_bytes(body).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return Err(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .header("content-type", "application/json")
                .header("access-control-allow-origin", "*")
                .body(Body::from(r#"{"error":"Failed to read request body"}"#))
                .unwrap());
        }
    };

    // Debug: log the received JSON for troubleshooting
    if let Ok(body_str) = std::str::from_utf8(&body_bytes) {
        if !body_str.is_empty() {
            sys_debug!("[REQUEST] Body: {}", body_str);
        }
    }

    // Deserialize JSON
    match serde_json::from_slice::<T>(&body_bytes) {
        Ok(parsed) => Ok(parsed),
        Err(e) => {
            sys_error!("[REQUEST] JSON parsing error: {}", e);
            Err(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .header("content-type", "application/json")
                .header("access-control-allow-origin", "*")
                .body(Body::from(r#"{"error":"Invalid JSON format"}"#))
                .unwrap())
        }
    }
}

/// Extract a query parameter from URI.
///
/// Returns `Some(value)` if the parameter exists, `None` otherwise.
/// The value is URL-decoded automatically.
///
/// # Example
/// ```
/// // For URI: /api/model/info?path=/models/llama.gguf
/// let model_path = get_query_param(req.uri(), "path");
/// ```
/// Extract query parameter from URI
/// TODO: Use for pagination, filtering, or search parameters
#[allow(dead_code)]
pub fn get_query_param(uri: &Uri, key: &str) -> Option<String> {
    let query = uri.query()?;

    for param in query.split('&') {
        if let Some((param_key, param_value)) = param.split_once('=') {
            if param_key == key {
                // URL decode the value
                return urlencoding::decode(param_value)
                    .ok()
                    .map(|s| s.to_string());
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyper::Uri;

    #[test]
    fn test_get_query_param_basic() {
        let uri: Uri = "/api/test?foo=bar".parse().unwrap();
        assert_eq!(get_query_param(&uri, "foo"), Some("bar".to_string()));
    }

    #[test]
    fn test_get_query_param_url_encoded() {
        let uri: Uri = "/api/test?path=%2Fhome%2Fuser%2Fmodel.gguf".parse().unwrap();
        assert_eq!(get_query_param(&uri, "path"), Some("/home/user/model.gguf".to_string()));
    }

    #[test]
    fn test_get_query_param_multiple_params() {
        let uri: Uri = "/api/test?foo=bar&baz=qux&name=test".parse().unwrap();
        assert_eq!(get_query_param(&uri, "foo"), Some("bar".to_string()));
        assert_eq!(get_query_param(&uri, "baz"), Some("qux".to_string()));
        assert_eq!(get_query_param(&uri, "name"), Some("test".to_string()));
    }

    #[test]
    fn test_get_query_param_not_found() {
        let uri: Uri = "/api/test?foo=bar".parse().unwrap();
        assert_eq!(get_query_param(&uri, "missing"), None);
    }

    #[test]
    fn test_get_query_param_no_query() {
        let uri: Uri = "/api/test".parse().unwrap();
        assert_eq!(get_query_param(&uri, "foo"), None);
    }

    #[test]
    fn test_get_query_param_empty_value() {
        let uri: Uri = "/api/test?foo=".parse().unwrap();
        assert_eq!(get_query_param(&uri, "foo"), Some("".to_string()));
    }

    #[test]
    fn test_get_query_param_spaces() {
        let uri: Uri = "/api/test?message=hello%20world".parse().unwrap();
        assert_eq!(get_query_param(&uri, "message"), Some("hello world".to_string()));
    }
}
