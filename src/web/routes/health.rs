// Health check route handler

use hyper::{Body, Response, StatusCode};
use std::convert::Infallible;

use crate::web::response_helpers::json_raw;

#[cfg(not(feature = "mock"))]
use crate::web::worker::worker_bridge::SharedWorkerBridge;

pub async fn handle(
    #[cfg(not(feature = "mock"))] _bridge: SharedWorkerBridge,
    #[cfg(feature = "mock")] _bridge: (),
) -> Result<Response<Body>, Infallible> {
    Ok(json_raw(
        StatusCode::OK,
        r#"{"status":"ok","service":"llama-chat-web"}"#.to_string(),
    ))
}
