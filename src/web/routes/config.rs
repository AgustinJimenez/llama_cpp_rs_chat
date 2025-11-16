// Configuration route handlers

use hyper::{Body, Request, Response, StatusCode};
use std::convert::Infallible;
use std::fs;

use crate::web::{
    models::SamplerConfig,
    config::load_config,
};

pub async fn handle_get_config(
    #[cfg(not(feature = "mock"))]
    _llama_state: crate::web::models::SharedLlamaState,
    #[cfg(feature = "mock")]
    _llama_state: (),
) -> Result<Response<Body>, Infallible> {
    // Load current configuration from file or use defaults
    let config = load_config();

    match serde_json::to_string(&config) {
        Ok(config_json) => {
            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .header("access-control-allow-origin", "*")
                .body(Body::from(config_json))
                .unwrap())
        }
        Err(_) => {
            Ok(Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .header("content-type", "application/json")
                .header("access-control-allow-origin", "*")
                .body(Body::from(r#"{"error":"Failed to serialize configuration"}"#))
                .unwrap())
        }
    }
}

pub async fn handle_post_config(
    req: Request<Body>,
    #[cfg(not(feature = "mock"))]
    _llama_state: crate::web::models::SharedLlamaState,
    #[cfg(feature = "mock")]
    _llama_state: (),
) -> Result<Response<Body>, Infallible> {
    // Parse request body for configuration update
    let body_bytes = match hyper::body::to_bytes(req.into_body()).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .header("content-type", "application/json")
                .header("access-control-allow-origin", "*")
                .body(Body::from(r#"{"error":"Failed to read request body"}"#))
                .unwrap());
        }
    };

    // Parse incoming config
    let incoming_config: SamplerConfig = match serde_json::from_slice(&body_bytes) {
        Ok(config) => config,
        Err(_) => {
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .header("content-type", "application/json")
                .header("access-control-allow-origin", "*")
                .body(Body::from(r#"{"error":"Invalid JSON format"}"#))
                .unwrap());
        }
    };

    // Load existing config to preserve model_history
    let mut existing_config = load_config();

    // Update fields from incoming config, but preserve model_history
    existing_config.sampler_type = incoming_config.sampler_type;
    existing_config.temperature = incoming_config.temperature;
    existing_config.top_p = incoming_config.top_p;
    existing_config.top_k = incoming_config.top_k;
    existing_config.mirostat_tau = incoming_config.mirostat_tau;
    existing_config.mirostat_eta = incoming_config.mirostat_eta;
    existing_config.model_path = incoming_config.model_path;
    existing_config.system_prompt = incoming_config.system_prompt;
    existing_config.context_size = incoming_config.context_size;
    existing_config.stop_tokens = incoming_config.stop_tokens;
    // Note: model_history is NOT updated from incoming config

    // Save merged configuration to file
    let config_path = "assets/config.json";
    if let Err(_) = fs::create_dir_all("assets") {
        return Ok(Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .header("content-type", "application/json")
            .header("access-control-allow-origin", "*")
            .body(Body::from(r#"{"error":"Failed to create config directory"}"#))
            .unwrap());
    }

    match fs::write(config_path, serde_json::to_string_pretty(&existing_config).unwrap_or_default()) {
        Ok(_) => {
            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .header("access-control-allow-origin", "*")
                .body(Body::from(r#"{"success":true}"#))
                .unwrap())
        }
        Err(_) => {
            Ok(Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .header("content-type", "application/json")
                .header("access-control-allow-origin", "*")
                .body(Body::from(r#"{"error":"Failed to save configuration"}"#))
                .unwrap())
        }
    }
}
