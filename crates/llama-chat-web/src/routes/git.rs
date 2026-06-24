use hyper::{Body, Request, Response, StatusCode};
use std::convert::Infallible;
use std::process::Command;

use crate::response_helpers::{json_error, json_raw};

// ── Staging / commit helpers ──────────────────────────────────────────────────

#[derive(serde::Serialize)]
struct StatusEntry { status: String, path: String }

#[derive(serde::Serialize)]
struct GitStatusResult { staged: Vec<StatusEntry>, unstaged: Vec<StatusEntry> }

async fn read_json_body(req: Request<Body>) -> serde_json::Value {
    let bytes = hyper::body::to_bytes(req.into_body()).await.unwrap_or_default();
    serde_json::from_slice(&bytes).unwrap_or_default()
}

fn simple_ok() -> Response<Body> {
    json_raw(StatusCode::OK, r#"{"ok":true,"error":null}"#.to_string())
}

fn simple_err(msg: &str) -> Response<Body> {
    json_raw(StatusCode::OK, serde_json::json!({"ok":false,"error":msg}).to_string())
}

fn run_git_status(path: &str) -> Result<GitStatusResult, String> {
    let out = Command::new("git")
        .args(["-C", path, "status", "--porcelain=v1"])
        .output()
        .map_err(|e| format!("git not available: {e}"))?;
    if !out.status.success() {
        let s = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if s.is_empty() { "git status failed".into() } else { s });
    }
    let mut staged: Vec<StatusEntry> = vec![];
    let mut unstaged: Vec<StatusEntry> = vec![];
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        if line.len() < 3 { continue; }
        let x = line.chars().next().unwrap_or(' ');
        let y = line.chars().nth(1).unwrap_or(' ');
        let raw = &line[3..];
        let file_path = if raw.contains(" -> ") {
            raw.split(" -> ").last().unwrap_or(raw).trim().to_string()
        } else { raw.trim().to_string() };
        if x != ' ' && x != '?' {
            staged.push(StatusEntry { status: x.to_string(), path: file_path.clone() });
        }
        if y != ' ' && y != '?' {
            unstaged.push(StatusEntry { status: y.to_string(), path: file_path.clone() });
        } else if x == '?' && y == '?' {
            unstaged.push(StatusEntry { status: "?".to_string(), path: file_path });
        }
    }
    Ok(GitStatusResult { staged, unstaged })
}

fn run_git_simple(path: &str, args: &[&str]) -> Result<String, String> {
    let mut a = vec!["-C", path];
    a.extend_from_slice(args);
    let out = Command::new("git").args(&a).output().map_err(|e| format!("git: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    if out.status.success() {
        Ok(if stdout.is_empty() { if stderr.is_empty() { "Done".into() } else { stderr } } else { stdout })
    } else {
        Err(if stderr.is_empty() { stdout } else { stderr })
    }
}

/// GET /api/git/status?path=
pub async fn handle_git_status(req: Request<Body>) -> Result<Response<Body>, Infallible> {
    let query = req.uri().query().unwrap_or("").to_owned();
    let path = query.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        if k == "path" { urlencoding::decode(v).ok().map(|s| s.into_owned()) } else { None }
    });
    let Some(path) = path else {
        return Ok(json_error(StatusCode::BAD_REQUEST, "path required"));
    };
    let result = tokio::task::spawn_blocking(move || run_git_status(&path))
        .await.unwrap_or_else(|e| Err(format!("task: {e}")));
    match result {
        Ok(s) => Ok(json_raw(StatusCode::OK, serde_json::json!({"staged":s.staged,"unstaged":s.unstaged,"error":null}).to_string())),
        Err(e) => Ok(json_raw(StatusCode::OK, serde_json::json!({"staged":[],"unstaged":[],"error":e}).to_string())),
    }
}

/// POST /api/git/stage  body: {path, files: []}  (empty files = stage all)
pub async fn handle_git_stage(req: Request<Body>) -> Result<Response<Body>, Infallible> {
    let body = read_json_body(req).await;
    let path = body["path"].as_str().unwrap_or("").to_string();
    let files: Vec<String> = body["files"].as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    if path.is_empty() { return Ok(json_error(StatusCode::BAD_REQUEST, "path required")); }
    let result = tokio::task::spawn_blocking(move || {
        if files.is_empty() {
            run_git_simple(&path, &["add", "-A"])
        } else {
            let mut args: Vec<String> = vec!["add".into(), "--".into()];
            args.extend(files);
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            run_git_simple(&path, &refs)
        }
    }).await.unwrap_or_else(|e| Err(format!("task: {e}")));
    match result {
        Ok(_) => Ok(simple_ok()),
        Err(e) => Ok(simple_err(&e)),
    }
}

/// POST /api/git/unstage  body: {path, files: []}  (empty = unstage all)
pub async fn handle_git_unstage(req: Request<Body>) -> Result<Response<Body>, Infallible> {
    let body = read_json_body(req).await;
    let path = body["path"].as_str().unwrap_or("").to_string();
    let files: Vec<String> = body["files"].as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    if path.is_empty() { return Ok(json_error(StatusCode::BAD_REQUEST, "path required")); }
    let result = tokio::task::spawn_blocking(move || {
        if files.is_empty() {
            run_git_simple(&path, &["reset", "HEAD"])
        } else {
            let mut args: Vec<String> = vec!["restore".into(), "--staged".into(), "--".into()];
            args.extend(files);
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            run_git_simple(&path, &refs)
        }
    }).await.unwrap_or_else(|e| Err(format!("task: {e}")));
    match result {
        Ok(_) => Ok(simple_ok()),
        Err(e) => Ok(simple_err(&e)),
    }
}

/// POST /api/git/commit  body: {path, message, description?}
pub async fn handle_git_commit_create(req: Request<Body>) -> Result<Response<Body>, Infallible> {
    let body = read_json_body(req).await;
    let path = body["path"].as_str().unwrap_or("").to_string();
    let message = body["message"].as_str().unwrap_or("").to_string();
    let description = body["description"].as_str().unwrap_or("").to_string();
    if path.is_empty() || message.is_empty() {
        return Ok(json_error(StatusCode::BAD_REQUEST, "path and message required"));
    }
    let result = tokio::task::spawn_blocking(move || {
        let full = if description.is_empty() { message } else { format!("{}\n\n{}", message, description) };
        run_git_simple(&path, &["commit", "-m", &full])
    }).await.unwrap_or_else(|e| Err(format!("task: {e}")));
    match result {
        Ok(out) => Ok(json_raw(StatusCode::OK, serde_json::json!({"ok":true,"output":out,"error":null}).to_string())),
        Err(e) => Ok(json_raw(StatusCode::OK, serde_json::json!({"ok":false,"output":"","error":e}).to_string())),
    }
}

/// POST /api/git/fetch  body: {path}
pub async fn handle_git_fetch(req: Request<Body>) -> Result<Response<Body>, Infallible> {
    let body = read_json_body(req).await;
    let path = body["path"].as_str().unwrap_or("").to_string();
    if path.is_empty() { return Ok(json_error(StatusCode::BAD_REQUEST, "path required")); }
    let result = tokio::task::spawn_blocking(move || run_git_simple(&path, &["fetch", "--all"]))
        .await.unwrap_or_else(|e| Err(format!("task: {e}")));
    match result {
        Ok(out) => Ok(json_raw(StatusCode::OK, serde_json::json!({"ok":true,"output":out,"error":null}).to_string())),
        Err(e) => Ok(json_raw(StatusCode::OK, serde_json::json!({"ok":false,"output":"","error":e}).to_string())),
    }
}

/// POST /api/git/pull  body: {path}
pub async fn handle_git_pull(req: Request<Body>) -> Result<Response<Body>, Infallible> {
    let body = read_json_body(req).await;
    let path = body["path"].as_str().unwrap_or("").to_string();
    if path.is_empty() { return Ok(json_error(StatusCode::BAD_REQUEST, "path required")); }
    let result = tokio::task::spawn_blocking(move || run_git_simple(&path, &["pull"]))
        .await.unwrap_or_else(|e| Err(format!("task: {e}")));
    match result {
        Ok(out) => Ok(json_raw(StatusCode::OK, serde_json::json!({"ok":true,"output":out,"error":null}).to_string())),
        Err(e) => Ok(json_raw(StatusCode::OK, serde_json::json!({"ok":false,"output":"","error":e}).to_string())),
    }
}

/// POST /api/git/push  body: {path}
pub async fn handle_git_push(req: Request<Body>) -> Result<Response<Body>, Infallible> {
    let body = read_json_body(req).await;
    let path = body["path"].as_str().unwrap_or("").to_string();
    if path.is_empty() { return Ok(json_error(StatusCode::BAD_REQUEST, "path required")); }
    let result = tokio::task::spawn_blocking(move || run_git_simple(&path, &["push"]))
        .await.unwrap_or_else(|e| Err(format!("task: {e}")));
    match result {
        Ok(out) => Ok(json_raw(StatusCode::OK, serde_json::json!({"ok":true,"output":out,"error":null}).to_string())),
        Err(e) => Ok(json_raw(StatusCode::OK, serde_json::json!({"ok":false,"output":"","error":e}).to_string())),
    }
}

/// POST /api/git/checkout  body: {path, hash}
pub async fn handle_git_checkout(req: Request<Body>) -> Result<Response<Body>, Infallible> {
    let body = read_json_body(req).await;
    let path = body["path"].as_str().unwrap_or("").to_string();
    let hash = body["hash"].as_str().unwrap_or("").to_string();
    if path.is_empty() || hash.is_empty() {
        return Ok(json_error(StatusCode::BAD_REQUEST, "path and hash required"));
    }
    let result = tokio::task::spawn_blocking(move || run_git_simple(&path, &["checkout", &hash]))
        .await.unwrap_or_else(|e| Err(format!("task: {e}")));
    match result {
        Ok(out) => Ok(json_raw(StatusCode::OK, serde_json::json!({"ok":true,"output":out,"error":null}).to_string())),
        Err(e) => Ok(json_raw(StatusCode::OK, serde_json::json!({"ok":false,"output":"","error":e}).to_string())),
    }
}

/// POST /api/git/stash-push  body: {path}
pub async fn handle_git_stash_push(req: Request<Body>) -> Result<Response<Body>, Infallible> {
    let body = read_json_body(req).await;
    let path = body["path"].as_str().unwrap_or("").to_string();
    if path.is_empty() { return Ok(json_error(StatusCode::BAD_REQUEST, "path required")); }
    let result = tokio::task::spawn_blocking(move || run_git_simple(&path, &["stash", "push"]))
        .await.unwrap_or_else(|e| Err(format!("task: {e}")));
    match result {
        Ok(out) => Ok(json_raw(StatusCode::OK, serde_json::json!({"ok":true,"output":out,"error":null}).to_string())),
        Err(e) => Ok(json_raw(StatusCode::OK, serde_json::json!({"ok":false,"output":"","error":e}).to_string())),
    }
}

/// POST /api/git/stash-pop  body: {path}
pub async fn handle_git_stash_pop(req: Request<Body>) -> Result<Response<Body>, Infallible> {
    let body = read_json_body(req).await;
    let path = body["path"].as_str().unwrap_or("").to_string();
    if path.is_empty() { return Ok(json_error(StatusCode::BAD_REQUEST, "path required")); }
    let result = tokio::task::spawn_blocking(move || run_git_simple(&path, &["stash", "pop"]))
        .await.unwrap_or_else(|e| Err(format!("task: {e}")));
    match result {
        Ok(out) => Ok(json_raw(StatusCode::OK, serde_json::json!({"ok":true,"output":out,"error":null}).to_string())),
        Err(e) => Ok(json_raw(StatusCode::OK, serde_json::json!({"ok":false,"output":"","error":e}).to_string())),
    }
}

/// POST /api/git/create-branch  body: {path, name, hash?}
pub async fn handle_git_create_branch(req: Request<Body>) -> Result<Response<Body>, Infallible> {
    let body = read_json_body(req).await;
    let path = body["path"].as_str().unwrap_or("").to_string();
    let name = body["name"].as_str().unwrap_or("").to_string();
    let hash = body["hash"].as_str().unwrap_or("").to_string();
    if path.is_empty() || name.is_empty() { return Ok(json_error(StatusCode::BAD_REQUEST, "path and name required")); }
    let result = tokio::task::spawn_blocking(move || {
        if hash.is_empty() {
            run_git_simple(&path, &["checkout", "-b", &name])
        } else {
            run_git_simple(&path, &["checkout", "-b", &name, &hash])
        }
    }).await.unwrap_or_else(|e| Err(format!("task: {e}")));
    match result {
        Ok(out) => Ok(json_raw(StatusCode::OK, serde_json::json!({"ok":true,"output":out,"error":null}).to_string())),
        Err(e) => Ok(json_raw(StatusCode::OK, serde_json::json!({"ok":false,"output":"","error":e}).to_string())),
    }
}

/// POST /api/git/revert  body: {path, hash}
pub async fn handle_git_revert(req: Request<Body>) -> Result<Response<Body>, Infallible> {
    let body = read_json_body(req).await;
    let path = body["path"].as_str().unwrap_or("").to_string();
    let hash = body["hash"].as_str().unwrap_or("").to_string();
    if path.is_empty() || hash.is_empty() { return Ok(json_error(StatusCode::BAD_REQUEST, "path and hash required")); }
    let result = tokio::task::spawn_blocking(move || run_git_simple(&path, &["revert", &hash, "--no-edit"]))
        .await.unwrap_or_else(|e| Err(format!("task: {e}")));
    match result {
        Ok(out) => Ok(json_raw(StatusCode::OK, serde_json::json!({"ok":true,"output":out,"error":null}).to_string())),
        Err(e) => Ok(json_raw(StatusCode::OK, serde_json::json!({"ok":false,"output":"","error":e}).to_string())),
    }
}

/// POST /api/git/reset  body: {path, mode, hash}  mode: soft|mixed|hard
pub async fn handle_git_reset(req: Request<Body>) -> Result<Response<Body>, Infallible> {
    let body = read_json_body(req).await;
    let path = body["path"].as_str().unwrap_or("").to_string();
    let mode = body["mode"].as_str().unwrap_or("mixed").to_string();
    let hash = body["hash"].as_str().unwrap_or("").to_string();
    if path.is_empty() || hash.is_empty() { return Ok(json_error(StatusCode::BAD_REQUEST, "path and hash required")); }
    let flag = match mode.as_str() { "soft" => "--soft", "hard" => "--hard", _ => "--mixed" };
    let result = tokio::task::spawn_blocking(move || run_git_simple(&path, &["reset", flag, &hash]))
        .await.unwrap_or_else(|e| Err(format!("task: {e}")));
    match result {
        Ok(out) => Ok(json_raw(StatusCode::OK, serde_json::json!({"ok":true,"output":out,"error":null}).to_string())),
        Err(e) => Ok(json_raw(StatusCode::OK, serde_json::json!({"ok":false,"output":"","error":e}).to_string())),
    }
}

/// POST /api/git/cherry-pick  body: {path, hash}
pub async fn handle_git_cherry_pick(req: Request<Body>) -> Result<Response<Body>, Infallible> {
    let body = read_json_body(req).await;
    let path = body["path"].as_str().unwrap_or("").to_string();
    let hash = body["hash"].as_str().unwrap_or("").to_string();
    if path.is_empty() || hash.is_empty() { return Ok(json_error(StatusCode::BAD_REQUEST, "path and hash required")); }
    let result = tokio::task::spawn_blocking(move || run_git_simple(&path, &["cherry-pick", &hash]))
        .await.unwrap_or_else(|e| Err(format!("task: {e}")));
    match result {
        Ok(out) => Ok(json_raw(StatusCode::OK, serde_json::json!({"ok":true,"output":out,"error":null}).to_string())),
        Err(e) => Ok(json_raw(StatusCode::OK, serde_json::json!({"ok":false,"output":"","error":e}).to_string())),
    }
}

/// POST /api/git/amend  body: {path, message?, description?}
pub async fn handle_git_amend(req: Request<Body>) -> Result<Response<Body>, Infallible> {
    let body = read_json_body(req).await;
    let path = body["path"].as_str().unwrap_or("").to_string();
    let message = body["message"].as_str().unwrap_or("").to_string();
    let description = body["description"].as_str().unwrap_or("").to_string();
    if path.is_empty() { return Ok(json_error(StatusCode::BAD_REQUEST, "path required")); }
    let result = tokio::task::spawn_blocking(move || {
        if message.is_empty() {
            run_git_simple(&path, &["commit", "--amend", "--no-edit"])
        } else {
            let full = if description.is_empty() { message.clone() } else { format!("{}\n\n{}", message, description) };
            run_git_simple(&path, &["commit", "--amend", "-m", &full])
        }
    }).await.unwrap_or_else(|e| Err(format!("task: {e}")));
    match result {
        Ok(out) => Ok(json_raw(StatusCode::OK, serde_json::json!({"ok":true,"output":out,"error":null}).to_string())),
        Err(e) => Ok(json_raw(StatusCode::OK, serde_json::json!({"ok":false,"output":"","error":e}).to_string())),
    }
}

#[derive(serde::Serialize)]
struct GitEntry {
    hash: String,
    short_hash: String,
    parents: Vec<String>,
    subject: String,
    body: String,
    refs: Vec<String>,
    date: String,
    author: String,
    author_email: String,
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
            "--format=%H\x1f%P\x1f%s\x1f%D\x1f%aI\x1f%an\x1f%ae\x1f%b\x1e",
            "--topo-order",
        ])
        .output()
        .map_err(|e| format!("git not available: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if stderr.is_empty() { "not a git repository".to_string() } else { stderr });
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .split('\x1e')
        .filter(|r| !r.trim().is_empty())
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
    let args: Vec<&str> = match hash {
        "WORKING" => vec!["-C", path, "diff", "HEAD", "--", file],
        "STAGED"  => vec!["-C", path, "diff", "--cached", "HEAD", "--", file],
        _         => vec!["-C", path, "show", hash, "--", file],
    };
    let out = Command::new("git")
        .args(&args)
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

fn parse_entry(record: &str) -> Option<GitEntry> {
    let mut parts = record.trim().splitn(8, '\x1f');
    let hash = parts.next()?.trim().to_string();
    let parents: Vec<String> = parts.next()?.split_whitespace().map(str::to_string).collect();
    let subject = parts.next()?.trim().to_string();
    let refs: Vec<String> = parts.next()?.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect();
    let date = parts.next()?.trim().to_string();
    let author = parts.next()?.trim().to_string();
    let author_email = parts.next()?.trim().to_string();
    let body_raw = parts.next().unwrap_or("").trim().to_string();
    let body = body_raw.lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with("Co-Authored-By:") && !l.starts_with("Claude-Session:") && !l.starts_with("Signed-off-by:"))
        .unwrap_or("")
        .to_string();
    let short_hash: String = hash.chars().take(7).collect();
    Some(GitEntry { hash, short_hash, parents, subject, body, refs, date, author, author_email })
}
