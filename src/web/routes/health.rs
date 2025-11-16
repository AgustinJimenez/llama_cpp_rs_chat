// Health check route handler

use hyper::{Body, Response};
use std::convert::Infallible;

use crate::web::response::json_ok;

#[cfg(not(feature = "mock"))]
use crate::web::models::SharedLlamaState;

pub async fn handle(
    #[cfg(not(feature = "mock"))]
    _llama_state: SharedLlamaState,
    #[cfg(feature = "mock")]
    _llama_state: (),
) -> Result<Response<Body>, Infallible> {
    Ok(json_ok(r#"{"status":"ok","service":"llama-chat-web"}"#))
}
