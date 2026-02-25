/// HuggingFace Hub proxy — searches for GGUF models.
///
/// GET /api/hub/search?q=devstral&limit=20
/// Proxies the HuggingFace API to avoid CORS issues from the frontend.

use std::convert::Infallible;

use hyper::{Body, Request, Response, StatusCode};
use serde::{Deserialize, Serialize};
use tokio::task::spawn_blocking;

use crate::web::response_helpers::{json_error, json_response};

const HF_API: &str = "https://huggingface.co/api/models";

#[derive(Deserialize)]
struct HfSibling {
    rfilename: String,
    #[serde(default)]
    size: Option<u64>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct HfModel {
    id: String,
    #[serde(default, rename = "modelId")]
    model_id: Option<String>,
    #[serde(default)]
    author: Option<String>,
    #[serde(default)]
    downloads: u64,
    #[serde(default)]
    likes: u64,
    #[serde(default, rename = "lastModified")]
    last_modified: Option<String>,
    #[serde(default)]
    pipeline_tag: Option<String>,
    #[serde(default)]
    siblings: Vec<HfSibling>,
}

#[derive(Serialize)]
pub struct HubFile {
    name: String,
    size: u64,
}

#[derive(Serialize)]
pub struct HubModel {
    id: String,
    author: String,
    downloads: u64,
    likes: u64,
    last_modified: String,
    pipeline_tag: String,
    files: Vec<HubFile>,
}

fn extract_query_param(uri: &hyper::Uri, key: &str) -> Option<String> {
    uri.query()?
        .split('&')
        .find_map(|pair| {
            let (k, v) = pair.split_once('=')?;
            if k == key { Some(urlencoding::decode(v).unwrap_or_default().into_owned()) } else { None }
        })
}

const VALID_SORTS: &[&str] = &["downloads", "likes", "lastModified", "createdAt"];

fn search_hf(query: &str, limit: usize, sort: &str) -> Result<Vec<HubModel>, String> {
    let sort_field = if VALID_SORTS.contains(&sort) { sort } else { "downloads" };
    let url = format!(
        "{}?search={}&filter=gguf&pipeline_tag=text-generation&sort={}&direction=-1&limit={}&expand[]=siblings",
        HF_API,
        urlencoding::encode(query),
        sort_field,
        limit,
    );

    let resp = ureq::get(&url)
        .call()
        .map_err(|e| format!("HuggingFace API error: {e}"))?;

    let body = resp
        .into_string()
        .map_err(|e| format!("Failed to read HF response: {e}"))?;

    let models: Vec<HfModel> = serde_json::from_str(&body)
        .map_err(|e| format!("Failed to parse HF response: {e}"))?;

    Ok(models
        .into_iter()
        .map(|m| {
            let files: Vec<HubFile> = m
                .siblings
                .into_iter()
                .filter(|s| s.rfilename.ends_with(".gguf"))
                .map(|s| HubFile { name: s.rfilename, size: s.size.unwrap_or(0) })
                .collect();
            HubModel {
                id: m.id,
                author: m.author.unwrap_or_default(),
                downloads: m.downloads,
                likes: m.likes,
                last_modified: m.last_modified.unwrap_or_default(),
                pipeline_tag: m.pipeline_tag.unwrap_or_default(),
                files,
            }
        })
        .collect())
}

// ─── Tree endpoint: fetch file sizes for a single model ──────────────

#[derive(Deserialize)]
struct HfTreeEntry {
    #[serde(rename = "type")]
    entry_type: String,
    #[serde(default)]
    path: String,
    #[serde(default)]
    size: u64,
}

fn tree_hf(model_id: &str) -> Result<Vec<HubFile>, String> {
    // model_id is like "user/repo" — keep the slash, only encode each segment
    let url = format!(
        "https://huggingface.co/api/models/{}/tree/main",
        model_id,
    );
    let resp = ureq::get(&url)
        .call()
        .map_err(|e| format!("HuggingFace API error: {e}"))?;
    let body = resp
        .into_string()
        .map_err(|e| format!("Failed to read HF tree response: {e}"))?;
    let entries: Vec<HfTreeEntry> = serde_json::from_str(&body)
        .map_err(|e| format!("Failed to parse HF tree response: {e}"))?;

    Ok(entries
        .into_iter()
        .filter(|e| e.entry_type == "file" && e.path.ends_with(".gguf"))
        .map(|e| HubFile { name: e.path, size: e.size })
        .collect())
}

pub async fn handle_tree(req: Request<Body>) -> Result<Response<Body>, Infallible> {
    let model_id = extract_query_param(req.uri(), "id").unwrap_or_default();
    if model_id.is_empty() {
        return Ok(json_error(StatusCode::BAD_REQUEST, "Missing ?id= parameter"));
    }
    match spawn_blocking(move || tree_hf(&model_id)).await {
        Ok(Ok(files)) => Ok(json_response(StatusCode::OK, &files)),
        Ok(Err(e)) => Ok(json_error(StatusCode::BAD_GATEWAY, &e)),
        Err(e) => Ok(json_error(StatusCode::INTERNAL_SERVER_ERROR, &format!("Task failed: {e}"))),
    }
}

// ─── Search endpoint ─────────────────────────────────────────────────

pub async fn handle_search(req: Request<Body>) -> Result<Response<Body>, Infallible> {
    let query = extract_query_param(req.uri(), "q").unwrap_or_default();
    let sort = extract_query_param(req.uri(), "sort").unwrap_or_else(|| "downloads".into());

    let limit: usize = extract_query_param(req.uri(), "limit")
        .and_then(|v| v.parse().ok())
        .unwrap_or(20)
        .min(50);

    match spawn_blocking(move || search_hf(&query, limit, &sort)).await {
        Ok(Ok(models)) => Ok(json_response(StatusCode::OK, &models)),
        Ok(Err(e)) => Ok(json_error(StatusCode::BAD_GATEWAY, &e)),
        Err(e) => Ok(json_error(StatusCode::INTERNAL_SERVER_ERROR, &format!("Task failed: {e}"))),
    }
}
