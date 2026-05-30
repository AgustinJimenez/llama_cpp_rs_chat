use super::*;

pub async fn handle_get_model_history(
    #[cfg(not(feature = "mock"))] _bridge: SharedWorkerBridge,
    #[cfg(feature = "mock")] _bridge: (),
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    let history = db.get_model_history().unwrap_or_default();
    let response_json = serialize_with_fallback(&history, "[]");
    Ok(json_raw(StatusCode::OK, response_json))
}

pub async fn handle_post_model_history(
    req: Request<Body>,
    #[cfg(not(feature = "mock"))] _bridge: SharedWorkerBridge,
    #[cfg(feature = "mock")] _bridge: (),
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    #[derive(Deserialize)]
    struct AddHistoryRequest {
        model_path: String,
    }

    let request: AddHistoryRequest = match parse_json_body(req.into_body()).await {
        Ok(req) => req,
        Err(error_response) => return Ok(error_response),
    };

    add_to_model_history(&db, &request.model_path);
    Ok(json_raw(StatusCode::OK, r#"{"success":true}"#.to_string()))
}

pub async fn handle_post_model_load(
    req: Request<Body>,
    #[cfg(not(feature = "mock"))] bridge: SharedWorkerBridge,
    #[cfg(feature = "mock")] _bridge: (),
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    sys_debug!("[DEBUG] /api/model/load endpoint hit");

    #[cfg(not(feature = "mock"))]
    {
        let load_request: ModelLoadRequest = match parse_json_body(req.into_body()).await {
            Ok(req) => req,
            Err(error_response) => return Ok(error_response),
        };

        match bridge
            .load_model(
                &load_request.model_path,
                load_request.gpu_layers,
                load_request.mmproj_path,
            )
            .await
        {
            Ok(meta) => {
                add_to_model_history(&db, &load_request.model_path);
                let tags = Some(get_tool_tags_for_model(meta.general_name.as_deref()));
                let status = llama_chat_types::models::ModelStatus {
                    loaded: true,
                    loading: None,
                    loading_progress: None,
                    generating: None,
                    active_conversation_id: None,
                    status_message: None,
                    model_path: Some(meta.model_path),
                    last_used: None,
                    memory_usage_mb: Some(512),
                    has_vision: Some(meta.has_vision),
                    tool_tags: tags,
                    gpu_layers: meta.gpu_layers,
                    block_count: meta.block_count,
                    system_prompt_tokens: None,
                    tool_definitions_tokens: None,
                    context_size: None,
                    last_finish_reason: None,
                    supports_thinking: Some(meta.supports_thinking),
                };
                let response = ModelResponse {
                    success: true,
                    message: format!("Model loaded successfully from {}", load_request.model_path),
                    status: Some(status),
                };

                let response_json = serialize_with_fallback(
                    &response,
                    r#"{"success":true,"message":"Model loaded successfully","status":null}"#,
                );

                Ok(json_raw(StatusCode::OK, response_json))
            }
            Err(e) => {
                let response = ModelResponse {
                    success: false,
                    message: format!("Failed to load model: {e}"),
                    status: None,
                };
                let response_json = serialize_with_fallback(
                    &response,
                    &format!(
                        r#"{{"success":false,"message":"Failed to load model: {e}","status":null}}"#
                    ),
                );

                Ok(json_raw(StatusCode::INTERNAL_SERVER_ERROR, response_json))
            }
        }
    }

    #[cfg(feature = "mock")]
    {
        let _ = req;
        Ok(json_raw(
            StatusCode::SERVICE_UNAVAILABLE,
            r#"{"success":false,"message":"Model loading not available (mock feature enabled)"}"#
                .to_string(),
        ))
    }
}

pub async fn handle_post_model_unload(
    #[cfg(not(feature = "mock"))] bridge: SharedWorkerBridge,
    #[cfg(feature = "mock")] _bridge: (),
) -> Result<Response<Body>, Infallible> {
    #[cfg(not(feature = "mock"))]
    {
        match bridge.force_unload().await {
            Ok(_) => {
                let status = llama_chat_types::models::ModelStatus {
                    loaded: false,
                    loading: None,
                    loading_progress: None,
                    generating: None,
                    active_conversation_id: None,
                    status_message: None,
                    model_path: None,
                    last_used: None,
                    memory_usage_mb: None,
                    has_vision: None,
                    tool_tags: None,
                    gpu_layers: None,
                    block_count: None,
                    system_prompt_tokens: None,
                    tool_definitions_tokens: None,
                    context_size: None,
                    last_finish_reason: None,
                    supports_thinking: None,
                };
                let response = ModelResponse {
                    success: true,
                    message: "Model unloaded successfully".to_string(),
                    status: Some(status),
                };
                let response_json = serialize_with_fallback(
                    &response,
                    r#"{"success":true,"message":"Model unloaded successfully","status":null}"#,
                );
                Ok(json_raw(StatusCode::OK, response_json))
            }
            Err(e) => {
                let response = ModelResponse {
                    success: false,
                    message: format!("Failed to unload model: {e}"),
                    status: None,
                };
                let response_json = serialize_with_fallback(
                    &response,
                    &format!(
                        r#"{{"success":false,"message":"Failed to unload model: {e}","status":null}}"#
                    ),
                );
                Ok(json_raw(StatusCode::INTERNAL_SERVER_ERROR, response_json))
            }
        }
    }

    #[cfg(feature = "mock")]
    {
        Ok(json_raw(
            StatusCode::SERVICE_UNAVAILABLE,
            r#"{"success":false,"message":"Model unloading not available (mock feature enabled)"}"#
                .to_string(),
        ))
    }
}

pub async fn handle_post_model_hard_unload(
    #[cfg(not(feature = "mock"))] bridge: SharedWorkerBridge,
    #[cfg(feature = "mock")] _bridge: (),
) -> Result<Response<Body>, Infallible> {
    #[cfg(not(feature = "mock"))]
    {
        match bridge.force_unload().await {
            Ok(_) => Ok(json_raw(
                StatusCode::OK,
                r#"{"success":true,"message":"Worker process killed, memory reclaimed"}"#
                    .to_string(),
            )),
            Err(e) => Ok(json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("Failed to force unload: {e}"),
            )),
        }
    }

    #[cfg(feature = "mock")]
    {
        Ok(json_raw(
            StatusCode::SERVICE_UNAVAILABLE,
            r#"{"success":false,"message":"Force unload not available (mock feature enabled)"}"#
                .to_string(),
        ))
    }
}

pub async fn handle_get_backends(
    #[cfg(not(feature = "mock"))] bridge: SharedWorkerBridge,
    #[cfg(feature = "mock")] _bridge: (),
) -> Result<Response<Body>, Infallible> {
    let nvidia_detected = detect_nvidia_gpu_hardware();

    #[cfg(not(feature = "mock"))]
    {
        match bridge.get_available_backends().await {
            Ok(backends) => {
                let has_cuda = backends.iter().any(|b| b.name == "CUDA" && b.available);
                let body = serde_json::json!({
                    "backends": backends,
                    "nvidia_gpu_detected": nvidia_detected,
                    "cuda_backend_loaded": has_cuda,
                });
                Ok(json_raw(
                    StatusCode::OK,
                    serde_json::to_string(&body).unwrap(),
                ))
            }
            Err(e) => Ok(json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("Failed to get backends: {e}"),
            )),
        }
    }

    #[cfg(feature = "mock")]
    {
        let body = serde_json::json!({
            "backends": [{"name":"CPU","available":true,"devices":[{"name":"CPU","description":"CPU"}]}],
            "nvidia_gpu_detected": nvidia_detected,
            "cuda_backend_loaded": false,
        });
        Ok(json_raw(
            StatusCode::OK,
            serde_json::to_string(&body).unwrap(),
        ))
    }
}
