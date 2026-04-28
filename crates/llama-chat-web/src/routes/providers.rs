// Provider route handlers

use hyper::{Body, Response, StatusCode};
use std::convert::Infallible;

use crate::providers;
use crate::response_helpers::{json_error, json_raw, serialize_with_fallback};

#[derive(serde::Deserialize)]
struct ProviderRequest {
    prompt: String,
    model: Option<String>,
    max_turns: Option<u32>,
    cwd: Option<String>,
    session_id: Option<String>,
    conversation_id: Option<String>,
}

/// GET /api/providers — list available providers and their status
pub async fn handle_list_providers(
    db: llama_chat_db::SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    let api_keys = load_api_keys_json(&db);
    let response = serde_json::json!({
        "providers": providers::list_providers_with_keys(api_keys.as_deref()).await
    });

    let response_json = serialize_with_fallback(&response, "{}");
    Ok(json_raw(StatusCode::OK, response_json))
}

async fn parse_provider_request(req: hyper::Request<Body>) -> Result<ProviderRequest, Response<Body>> {
    let body = hyper::body::to_bytes(req.into_body())
        .await
        .map_err(|_| json_error(StatusCode::BAD_REQUEST, "Failed to read body"))?;

    serde_json::from_slice(&body)
        .map_err(|e| json_error(StatusCode::BAD_REQUEST, &format!("Invalid JSON: {e}")))
}

fn display_model_name(provider_id: &str, model: Option<&str>) -> String {
    match provider_id {
        "claude_code" => providers::claude_code::display_model_name(model),
        "codex" => providers::codex::display_model_name(model),
        _ => {
            // For OpenAI-compat providers, use model name or provider default
            model
                .filter(|m| !m.is_empty())
                .map(|m| m.to_string())
                .unwrap_or_else(|| providers::default_model(provider_id).to_string())
        }
    }
}

fn load_api_keys_json(db: &llama_chat_db::SharedDatabase) -> Option<String> {
    let conn = db.connection();
    conn.query_row(
        "SELECT provider_api_keys FROM config WHERE id = 1",
        [],
        |row| row.get::<_, Option<String>>(0),
    )
    .ok()
    .flatten()
}

fn ensure_conversation(
    db: &llama_chat_db::SharedDatabase,
    conv_id: &str,
    now: u64,
) {
    let conn = db.connection();
    let _ = conn.execute(
        "INSERT OR IGNORE INTO conversations (id, created_at, updated_at, system_prompt, title, provider_id, provider_session_id)
         VALUES (?1, ?2, ?3, NULL, NULL, NULL, NULL)",
        rusqlite::params![conv_id, now as i64, now as i64],
    );
}

fn save_provider_turn(
    db: &llama_chat_db::SharedDatabase,
    conv_id: &str,
    provider_id: &str,
    provider_session_id: Option<&str>,
    prompt: &str,
    full_response: &str,
    now: u64,
) {
    if full_response.is_empty() {
        return;
    }

    ensure_conversation(db, conv_id, now);
    let next_seq = db
        .get_messages(conv_id)
        .map(|msgs| msgs.len() as i32 + 1)
        .unwrap_or(1);
    let _ = db.insert_message(conv_id, "user", prompt, now, next_seq);
    let _ = db.insert_message(conv_id, "assistant", full_response, now, next_seq + 1);
    let _ = db.set_conversation_provider_session_id(conv_id, Some(provider_id), provider_session_id);
    let conn = db.connection();
    let _ = conn.execute(
        "UPDATE conversations SET updated_at = ?1 WHERE id = ?2",
        rusqlite::params![now as i64, conv_id],
    );
}

/// POST /api/providers/{provider}/stream — streaming generation via CLI provider (SSE)
pub async fn handle_provider_stream(
    req: hyper::Request<Body>,
    db: llama_chat_db::SharedDatabase,
    provider_id: &str,
) -> Result<Response<Body>, Infallible> {
    let request = match parse_provider_request(req).await {
        Ok(r) => r,
        Err(resp) => return Ok(resp),
    };
    let provider_prompt = providers::compose_prompt(
        provider_id,
        &request.prompt,
        request.session_id.as_deref(),
    );

    let conv_id = request.conversation_id.unwrap_or_else(|| {
        format!("chat_{}", chrono::Local::now().format("%Y-%m-%d-%H-%M-%S-%3f"))
    });

    let api_keys = load_api_keys_json(&db);
    let mut rx = match providers::generate(
        provider_id,
        &provider_prompt,
        request.model.as_deref(),
        request.max_turns,
        request.cwd.as_deref(),
        request.session_id.as_deref(),
        api_keys.as_deref(),
        Some(&conv_id),
        Some(&db),
    )
    .await
    {
        Ok(rx) => rx,
        Err(e) => {
            return Ok(json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("Failed: {e}"),
            ))
        }
    };
    let prompt = request.prompt.clone();
    let provider_name = provider_id.to_string();

    let (mut sse_tx, sse_body) = Body::channel();
    let db_clone = db.clone();
    let conv_id_clone = conv_id.clone();

    tokio::spawn(async move {
        let mut full_response = String::new();

        while let Some(token_data) = rx.recv().await {
            if token_data.is_done {
                let done_json = serde_json::json!({
                    "type": "done",
                    "provider": provider_name,
                    "session_id": token_data.session_id,
                    "stop_reason": token_data.stop_reason,
                    "cost_usd": token_data.cost_usd,
                    "duration_ms": token_data.duration_ms,
                    "input_tokens": token_data.input_tokens,
                    "output_tokens": token_data.output_tokens,
                    "model": token_data.model_id,
                    "conversation_id": conv_id_clone,
                });
                let _ = sse_tx
                    .send_data(hyper::body::Bytes::from(format!("data: {}\n\n", done_json)))
                    .await;

                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                save_provider_turn(
                    &db_clone,
                    &conv_id_clone,
                    &provider_name,
                    token_data.session_id.as_deref(),
                    &prompt,
                    &full_response,
                    now,
                );
                break;
            }

            if token_data.token.is_empty() && (token_data.input_tokens.is_some() || token_data.duration_ms.is_some()) {
                // Status update (cumulative token tracking, no visible text)
                let status_json = serde_json::json!({
                    "type": "status",
                    "input_tokens": token_data.input_tokens,
                    "output_tokens": token_data.output_tokens,
                    "duration_ms": token_data.duration_ms,
                });
                let _ = sse_tx
                    .send_data(hyper::body::Bytes::from(format!("data: {}\n\n", status_json)))
                    .await;
                continue;
            }

            full_response.push_str(&token_data.token);

            let token_json = serde_json::json!({
                "type": "token",
                "token": token_data.token,
            });
            if sse_tx
                .send_data(hyper::body::Bytes::from(format!("data: {}\n\n", token_json)))
                .await
                .is_err()
            {
                break;
            }
        }
    });

    let response = Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/event-stream")
        .header("Cache-Control", "no-cache")
        .header("Connection", "keep-alive")
        .body(sse_body)
        .unwrap();

    Ok(response)
}

/// POST /api/providers/{provider}/generate — non-streaming generation via CLI provider
pub async fn handle_provider_generate(
    req: hyper::Request<Body>,
    db: llama_chat_db::SharedDatabase,
    provider_id: &str,
) -> Result<Response<Body>, Infallible> {
    let request = match parse_provider_request(req).await {
        Ok(r) => r,
        Err(resp) => return Ok(resp),
    };
    let provider_prompt = providers::compose_prompt(
        provider_id,
        &request.prompt,
        request.session_id.as_deref(),
    );

    let conv_id_for_history = request.conversation_id.as_deref().unwrap_or("");
    let api_keys = load_api_keys_json(&db);
    let mut rx = match providers::generate(
        provider_id,
        &provider_prompt,
        request.model.as_deref(),
        request.max_turns,
        request.cwd.as_deref(),
        request.session_id.as_deref(),
        api_keys.as_deref(),
        if conv_id_for_history.is_empty() { None } else { Some(conv_id_for_history) },
        Some(&db),
    )
    .await
    {
        Ok(rx) => rx,
        Err(e) => {
            return Ok(json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("Failed to start provider: {e}"),
            ))
        }
    };

    let mut full_response = String::new();
    let mut cost_usd = None;
    let mut duration_ms = None;
    let mut stop_reason = None;
    let mut actual_model_id = None;
    let mut session_id = None;
    let mut input_tokens = None;
    let mut output_tokens = None;

    while let Some(token_data) = rx.recv().await {
        if token_data.model_id.is_some() {
            actual_model_id = token_data.model_id.clone();
        }
        if token_data.session_id.is_some() {
            session_id = token_data.session_id.clone();
        }
        if token_data.is_done {
            cost_usd = token_data.cost_usd;
            duration_ms = token_data.duration_ms;
            stop_reason = token_data.stop_reason;
            input_tokens = token_data.input_tokens;
            output_tokens = token_data.output_tokens;
            break;
        }
        full_response.push_str(&token_data.token);
    }

    let conv_id = request.conversation_id.unwrap_or_else(|| {
        format!("chat_{}", chrono::Local::now().format("%Y-%m-%d-%H-%M-%S-%3f"))
    });
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    save_provider_turn(
        &db,
        &conv_id,
        provider_id,
        session_id.as_deref(),
        &request.prompt,
        &full_response,
        now,
    );

    let display_model = actual_model_id
        .clone()
        .unwrap_or_else(|| display_model_name(provider_id, request.model.as_deref()));

    let result = serde_json::json!({
        "response": full_response,
        "cost_usd": cost_usd,
        "duration_ms": duration_ms,
        "stop_reason": stop_reason,
        "provider": provider_id,
        "model": display_model,
        "session_id": session_id,
        "conversation_id": conv_id,
        "input_tokens": input_tokens,
        "output_tokens": output_tokens,
    });

    let response_json = serialize_with_fallback(&result, "{}");
    Ok(json_raw(StatusCode::OK, response_json))
}

/// GET /api/providers/{provider}/models — fetch available models from provider
pub async fn handle_provider_models(
    provider_id: &str,
    db: llama_chat_db::SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    let api_keys = load_api_keys_json(&db);
    let api_key = match crate::providers::openai_compat::resolve_api_key(provider_id, api_keys.as_deref()) {
        Some(k) => k,
        None => return Ok(json_error(StatusCode::BAD_REQUEST, "No API key configured for this provider")),
    };
    let base_url = match crate::providers::openai_compat::resolve_base_url(provider_id, api_keys.as_deref()) {
        Some(u) => u,
        None => return Ok(json_error(StatusCode::BAD_REQUEST, "No base URL for this provider")),
    };

    let models = crate::providers::openai_compat::fetch_models(provider_id, &base_url, &api_key);
    let body = serde_json::json!({ "models": models });
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap())
}
