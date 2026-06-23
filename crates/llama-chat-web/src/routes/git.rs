use hyper::{Body, Request, Response, StatusCode};
use std::convert::Infallible;
use std::process::Command;

use crate::response_helpers::{json_error, json_raw};

#[derive(serde::Serialize)]
struct GitEntry {
    hash: String,
    short_hash: String,
    parents: Vec<String>,
    subject: String,
    refs: Vec<String>,
    date: String,
    author: String,
}

/// GET /api/git/log?path=<dir> — git log for the branch graph view.
pub async fn handle_git_log(req: Request<Body>) -> Result<Response<Body>, Infallible> {
    let query = req.uri().query().unwrap_or("").to_owned();
    let path = query.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        if k == "path" {
            urlencoding::decode(v).ok().map(|s| s.into_owned())
        } else {
            None
        }
    });
    let Some(path) = path else {
        return Ok(json_error(StatusCode::BAD_REQUEST, "path query param required"));
    };
    if !std::path::Path::new(&path).is_dir() {
        return Ok(json_raw(
            StatusCode::OK,
            serde_json::json!({"commits": [], "error": "not a directory"}).to_string(),
        ));
    }
    let result = tokio::task::spawn_blocking(move || run_git_log(&path))
        .await
        .unwrap_or_else(|e| Err(format!("task error: {e}")));
    match result {
        Ok(entries) => Ok(json_raw(
            StatusCode::OK,
            serde_json::json!({"commits": entries, "error": null}).to_string(),
        )),
        Err(e) => Ok(json_raw(
            StatusCode::OK,
            serde_json::json!({"commits": [], "error": e}).to_string(),
        )),
    }
}

fn run_git_log(path: &str) -> Result<Vec<GitEntry>, String> {
    let out = Command::new("git")
        .args([
            "-C",
            path,
            "log",
            "--all",
            "--format=%H|%P|%s|%D|%aI|%an",
            "--topo-order",
        ])
        .output()
        .map_err(|e| format!("git not available: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if stderr.is_empty() { "not a git repository".to_string() } else { stderr });
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| !l.is_empty())
        .filter_map(parse_entry)
        .collect())
}

#[derive(serde::Serialize)]
struct FileChange {
    status: String,
    path: String,
}

/// GET /api/git/commit-files?path=<dir>&hash=<hash> — changed files for a single commit.
pub async fn handle_git_commit_files(req: Request<Body>) -> Result<Response<Body>, Infallible> {
    let query = req.uri().query().unwrap_or("").to_owned();
    let mut path_opt: Option<String> = None;
    let mut hash_opt: Option<String> = None;
    for kv in query.split('&') {
        if let Some((k, v)) = kv.split_once('=') {
            match k {
                "path" => path_opt = urlencoding::decode(v).ok().map(|s| s.into_owned()),
                "hash" => hash_opt = urlencoding::decode(v).ok().map(|s| s.into_owned()),
                _ => {}
            }
        }
    }
    let (Some(path), Some(hash)) = (path_opt, hash_opt) else {
        return Ok(json_error(StatusCode::BAD_REQUEST, "path and hash required"));
    };
    if hash.len() < 7 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return Ok(json_error(StatusCode::BAD_REQUEST, "invalid hash"));
    }
    let result = tokio::task::spawn_blocking(move || run_git_commit_files(&path, &hash))
        .await
        .unwrap_or_else(|e| Err(format!("task error: {e}")));
    match result {
        Ok(files) => Ok(json_raw(
            StatusCode::OK,
            serde_json::json!({"files": files, "error": null}).to_string(),
        )),
        Err(e) => Ok(json_raw(
            StatusCode::OK,
            serde_json::json!({"files": [], "error": e}).to_string(),
        )),
    }
}

/// GET /api/git/file-diff?path=<dir>&hash=<hash>&file=<filepath>
pub async fn handle_git_file_diff(req: Request<Body>) -> Result<Response<Body>, Infallible> {
    let query = req.uri().query().unwrap_or("").to_owned();
    let mut path_opt: Option<String> = None;
    let mut hash_opt: Option<String> = None;
    let mut file_opt: Option<String> = None;
    for kv in query.split('&') {
        if let Some((k, v)) = kv.split_once('=') {
            match k {
                "path" => path_opt = urlencoding::decode(v).ok().map(|s| s.into_owned()),
                "hash" => hash_opt = urlencoding::decode(v).ok().map(|s| s.into_owned()),
                "file" => file_opt = urlencoding::decode(v).ok().map(|s| s.into_owned()),
                _ => {}
            }
        }
    }
    let (Some(path), Some(hash), Some(file)) = (path_opt, hash_opt, file_opt) else {
        return Ok(json_error(StatusCode::BAD_REQUEST, "path, hash, and file required"));
    };
    if hash.len() < 7 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return Ok(json_error(StatusCode::BAD_REQUEST, "invalid hash"));
    }
    if file.is_empty() || file.contains('\0') {
        return Ok(json_error(StatusCode::BAD_REQUEST, "invalid file path"));
    }
    let result = tokio::task::spawn_blocking(move || run_git_file_diff(&path, &hash, &file))
        .await
        .unwrap_or_else(|e| Err(format!("task error: {e}")));
    match result {
        Ok(diff) => Ok(json_raw(
            StatusCode::OK,
            serde_json::json!({"diff": diff, "error": null}).to_string(),
        )),
        Err(e) => Ok(json_raw(
            StatusCode::OK,
            serde_json::json!({"diff": "", "error": e}).to_string(),
        )),
    }
}

fn run_git_file_diff(path: &str, hash: &str, file: &str) -> Result<String, String> {
    let out = Command::new("git")
        .args(["-C", path, "show", hash, "--", file])
        .output()
        .map_err(|e| format!("git not available: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if stderr.is_empty() { "git show failed".into() } else { stderr });
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn run_git_commit_files(path: &str, hash: &str) -> Result<Vec<FileChange>, String> {
    let out = Command::new("git")
        .args(["-C", path, "show", "--name-status", "--format=", hash])
        .output()
        .map_err(|e| format!("git not available: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if stderr.is_empty() { "git show failed".into() } else { stderr });
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| !l.is_empty())
        .filter_map(|l| {
            let mut parts = l.splitn(2, '\t');
            let status = parts.next()?.trim().to_string();
            let file_path = parts.next()?.trim().to_string();
            Some(FileChange { status, path: file_path })
        })
        .collect())
}

fn parse_entry(line: &str) -> Option<GitEntry> {
    let mut parts = line.splitn(6, '|');
    let hash = parts.next()?.trim().to_string();
    let parents: Vec<String> = parts
        .next()?
        .split_whitespace()
        .map(str::to_string)
        .collect();
    let subject = parts.next()?.trim().to_string();
    let refs: Vec<String> = parts
        .next()?
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    let date = parts.next()?.trim().to_string();
    let author = parts.next()?.trim().to_string();
    let short_hash: String = hash.chars().take(7).collect();
    Some(GitEntry { hash, short_hash, parents, subject, refs, date, author })
}
