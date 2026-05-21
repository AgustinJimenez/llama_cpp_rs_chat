use std::convert::Infallible;
use std::time::UNIX_EPOCH;

use hyper::{Body, Request, Response, StatusCode};
use serde::{Deserialize, Serialize};

use crate::request_parsing::parse_json_body;
use crate::response_helpers::{json_error, json_response};
use crate::worker_pool::{
    remove_worker_and_rebind_conversations, WorkerEntry, WorkerPool,
};
use llama_chat_db::SharedDatabase;
use llama_chat_types::models::ModelLoadRequest;

#[derive(Serialize)]
struct WorkerSummary {
    id: String,
    created_at_secs: Option<u64>,
    loaded: bool,
    loading: bool,
    generating: bool,
    active_conversation_id: Option<String>,
    model_path: Option<String>,
    general_name: Option<String>,
    context_size: Option<u32>,
}

#[derive(Serialize)]
struct WorkersResponse {
    workers: Vec<WorkerSummary>,
}

#[derive(Serialize)]
struct WorkerCreateResponse {
    worker_id: String,
}

#[derive(Deserialize)]
struct UpdateConversationWorkerRequest {
    worker_id: Option<String>,
}

fn entry_created_at_secs(entry: &WorkerEntry) -> Option<u64> {
    entry.created_at
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs())
}

async fn summarize_worker(entry: WorkerEntry) -> WorkerSummary {
    let bridge = entry.bridge.clone();
    let is_loading = bridge.is_loading();
    let is_generating = bridge.is_generating().await;
    let active_conversation_id = bridge.active_conversation_id().await;
    let meta = bridge.model_status().await;
    let created_at_secs = entry_created_at_secs(&entry);
    let loaded = meta.as_ref().is_some_and(|m| m.loaded);
    let model_path = meta.as_ref().map(|m| m.model_path.clone());
    let general_name = meta.as_ref().and_then(|m| m.general_name.clone());
    let context_size = meta.as_ref().and_then(|m| m.context_length);

    WorkerSummary {
        id: entry.id,
        created_at_secs,
        loaded,
        loading: is_loading,
        generating: is_generating,
        active_conversation_id,
        model_path,
        general_name,
        context_size,
    }
}

pub async fn handle_list_workers(
    pool: WorkerPool,
) -> Result<Response<Body>, Infallible> {
    let mut summaries = Vec::new();
    for entry in pool.list_entries() {
        summaries.push(summarize_worker(entry).await);
    }

    summaries.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(json_response(
        StatusCode::OK,
        &WorkersResponse { workers: summaries },
    ))
}

pub async fn handle_create_worker(
    req: Request<Body>,
    pool: WorkerPool,
) -> Result<Response<Body>, Infallible> {
    let create: ModelLoadRequest = match parse_json_body(req.into_body()).await {
        Ok(body) => body,
        Err(response) => return Ok(response),
    };

    if create.model_path.trim().is_empty() {
        return Ok(json_error(StatusCode::BAD_REQUEST, "model_path is required"));
    }

    match pool
        .spawn_worker_with_options(
            &create.model_path,
            create.gpu_layers,
            create.mmproj_path,
        )
        .await
    {
        Ok(worker_id) => Ok(json_response(
            StatusCode::OK,
            &WorkerCreateResponse { worker_id },
        )),
        Err(e) => Ok(json_error(StatusCode::INTERNAL_SERVER_ERROR, &e)),
    }
}

pub async fn handle_delete_worker(
    worker_id: &str,
    pool: WorkerPool,
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    match remove_worker_and_rebind_conversations(&pool, &db, worker_id).await {
        Ok(()) => Ok(json_response(
            StatusCode::OK,
            &serde_json::json!({ "success": true, "worker_id": worker_id }),
        )),
        Err(e) => Ok(json_error(StatusCode::BAD_REQUEST, &e)),
    }
}

pub async fn handle_get_worker_status(
    worker_id: &str,
    pool: WorkerPool,
) -> Result<Response<Body>, Infallible> {
    let Some(entry) = pool
        .list_entries()
        .into_iter()
        .find(|entry| entry.id == worker_id)
    else {
        return Ok(json_error(StatusCode::NOT_FOUND, "Worker not found"));
    };

    Ok(json_response(StatusCode::OK, &summarize_worker(entry).await))
}

pub async fn handle_patch_conversation_worker(
    req: Request<Body>,
    conversation_id: &str,
    pool: WorkerPool,
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    let update: UpdateConversationWorkerRequest = match parse_json_body(req.into_body()).await {
        Ok(body) => body,
        Err(response) => return Ok(response),
    };

    let normalized_worker_id = update
        .worker_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty() && *id != "default")
        .map(str::to_string);

    if let Some(worker_id) = normalized_worker_id.as_deref() {
        if pool.get(worker_id).is_none() {
            return Ok(json_error(StatusCode::NOT_FOUND, "Worker not found"));
        }
    }

    match db.set_conversation_worker_id(conversation_id, normalized_worker_id.as_deref()) {
        Ok(()) => Ok(json_response(
            StatusCode::OK,
            &serde_json::json!({
                "success": true,
                "conversation_id": conversation_id,
                "worker_id": normalized_worker_id,
            }),
        )),
        Err(e) => Ok(json_error(StatusCode::INTERNAL_SERVER_ERROR, &e)),
    }
}
