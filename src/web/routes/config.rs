// Configuration route handlers

use hyper::{Body, Request, Response, StatusCode};
use std::convert::Infallible;
use std::fs;

use crate::web::{
    models::SamplerConfig,
    config::load_config,
    response_helpers::{json_error, json_raw},
    request_parsing::parse_json_body,
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
            Ok(json_raw(StatusCode::OK, config_json))
        }
        Err(_) => {
            Ok(json_error(StatusCode::INTERNAL_SERVER_ERROR, "Failed to serialize configuration"))
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
    // Parse request body using helper
    let incoming_config: SamplerConfig = match parse_json_body(req.into_body()).await {
        Ok(config) => config,
        Err(error_response) => return Ok(error_response),
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
        return Ok(json_error(StatusCode::INTERNAL_SERVER_ERROR, "Failed to create config directory"));
    }

    match fs::write(config_path, serde_json::to_string_pretty(&existing_config).unwrap_or_default()) {
        Ok(_) => {
            Ok(json_raw(StatusCode::OK, r#"{"success":true}"#.to_string()))
        }
        Err(_) => {
            Ok(json_error(StatusCode::INTERNAL_SERVER_ERROR, "Failed to save configuration"))
        }
    }
}
