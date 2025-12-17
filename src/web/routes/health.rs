// Health check route handler

use hyper::{Body, Response, StatusCode};
use std::convert::Infallible;

use crate::web::response_helpers::json_raw;

#[cfg(not(feature = "mock"))]
use crate::web::models::SharedLlamaState;

pub async fn handle(
    #[cfg(not(feature = "mock"))] _llama_state: SharedLlamaState,
    #[cfg(feature = "mock")] _llama_state: (),
) -> Result<Response<Body>, Infallible> {
    Ok(json_raw(
        StatusCode::OK,
        r#"{"status":"ok","service":"llama-chat-web"}"#.to_string(),
    ))
}
