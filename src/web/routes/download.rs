/// HuggingFace file download with SSE progress reporting and resume support.
///
/// POST /api/hub/download  { model_id, filename, destination }
/// Returns text/event-stream with progress, done, or error events.
/// Supports resuming interrupted downloads via HTTP Range requests and .part files.
///
/// GET  /api/hub/downloads         — list all download records
/// POST /api/hub/downloads/verify  — prune records whose files are missing, return clean list

use std::convert::Infallible;
use std::io::Read;
use std::path::{Path, PathBuf};

use hyper::{Body, Request, Response, StatusCode};
use serde::Deserialize;
use tokio::sync::mpsc;
use tokio::task::spawn_blocking;

use crate::web::database::SharedDatabase;
use crate::web::request_parsing::parse_json_body;
use crate::web::response_helpers::{json_error, json_raw};

#[derive(Deserialize)]
struct DownloadRequest {
    model_id: String,
    filename: String,
    destination: String,
}

/// Send a JSON SSE event via the mpsc channel (blocking).
fn send_event(tx: &mpsc::Sender<String>, json: serde_json::Value) {
    let _ = tx.blocking_send(json.to_string());
}

/// Interval between DB progress checkpoints (bytes)
const CHECKPOINT_INTERVAL: u64 = 5 * 1024 * 1024; // 5 MB

fn download_file_blocking(
    url: &str,
    dest_file: &Path,
    part_file: &Path,
    db: SharedDatabase,
    model_id: &str,
    filename: &str,
    dest_path: &str,
    tx: mpsc::Sender<String>,
) {
    // 1. Check for existing .part file and DB record for resume
    let existing_record = db
        .find_pending_download(model_id, filename, dest_path)
        .unwrap_or(None);
    let resume_offset: u64 = if part_file.exists() {
        std::fs::metadata(part_file)
            .map(|m| m.len())
            .unwrap_or(0)
    } else {
        0
    };
    let stored_etag = existing_record.as_ref().and_then(|r| r.etag.clone());
    let record_id = existing_record.as_ref().map(|r| r.id);

    // 2. HTTP request — with Range header if resuming
    let mut request = ureq::get(url)
        .set("User-Agent", "Mozilla/5.0 (compatible; LlamaChat/1.0)");
    if resume_offset > 0 {
        request = request.set("Range", &format!("bytes={resume_offset}-"));
    }

    let resp = match request.call() {
        Ok(r) => r,
        Err(e) => {
            send_event(
                &tx,
                serde_json::json!({"type": "error", "message": format!("Download failed: {e}")}),
            );
            return;
        }
    };

    let status = resp.status();
    let is_range_response = status == 206;

    // 3. ETag validation — if server's ETag differs from stored, restart
    let server_etag = resp.header("etag").map(|s| s.to_string());
    let must_restart = if is_range_response {
        if let (Some(stored), Some(server)) = (&stored_etag, &server_etag) {
            stored != server // ETag changed → file was updated upstream
        } else {
            false
        }
    } else {
        // Server returned 200 instead of 206 — doesn't support Range or offset was 0
        resume_offset > 0 // If we asked for Range but got 200, restart from 0
    };

    let actual_offset = if must_restart || !is_range_response {
        // Restart: delete stale .part file
        if part_file.exists() {
            let _ = std::fs::remove_file(part_file);
        }
        0
    } else {
        resume_offset
    };

    // 4. Total size — from Content-Length + offset for range, or Content-Length for full
    let content_length: u64 = resp
        .header("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let total = if is_range_response && !must_restart {
        content_length + actual_offset
    } else {
        content_length
    };

    // 5. Create or open .part file
    let mut file = if actual_offset > 0 {
        // Append to existing .part file
        match std::fs::OpenOptions::new().append(true).open(part_file) {
            Ok(f) => f,
            Err(e) => {
                send_event(
                    &tx,
                    serde_json::json!({"type": "error", "message": format!("Cannot open .part file: {e}")}),
                );
                return;
            }
        }
    } else {
        // Create fresh .part file
        match std::fs::File::create(part_file) {
            Ok(f) => f,
            Err(e) => {
                send_event(
                    &tx,
                    serde_json::json!({"type": "error", "message": format!("Cannot create file: {e}")}),
                );
                return;
            }
        }
    };

    // 6. Create or update DB record
    let db_id = if let Some(id) = record_id {
        // Update existing record with new etag if changed
        if server_etag.is_some() && server_etag != stored_etag {
            let _ = db.save_hub_download(
                model_id,
                filename,
                dest_path,
                total as i64,
                "pending",
                server_etag.as_deref(),
            );
        }
        id
    } else {
        db.save_hub_download(
            model_id,
            filename,
            dest_path,
            total as i64,
            "pending",
            server_etag.as_deref(),
        )
        .unwrap_or(0)
    };

    // 7. Stream in 64KB chunks
    let mut reader = resp.into_reader();
    let mut buf = [0u8; 65536];
    let mut downloaded: u64 = actual_offset;
    let mut last_progress = std::time::Instant::now();
    let mut last_checkpoint = downloaded;
    let start = std::time::Instant::now();

    // Send initial progress if resuming so UI updates immediately
    if actual_offset > 0 {
        send_event(
            &tx,
            serde_json::json!({
                "type": "progress",
                "bytes": downloaded,
                "total": total,
                "speed_kbps": 0,
            }),
        );
    }

    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => {
                // On error: leave .part file for resume, save progress to DB
                if db_id > 0 {
                    let _ = db.update_download_progress(db_id, downloaded as i64);
                }
                send_event(
                    &tx,
                    serde_json::json!({"type": "error", "message": format!("Read error: {e}")}),
                );
                return;
            }
        };

        if let Err(e) = std::io::Write::write_all(&mut file, &buf[..n]) {
            if db_id > 0 {
                let _ = db.update_download_progress(db_id, downloaded as i64);
            }
            send_event(
                &tx,
                serde_json::json!({"type": "error", "message": format!("Write error: {e}")}),
            );
            return;
        }

        downloaded += n as u64;

        // DB checkpoint every 5MB
        if db_id > 0 && downloaded - last_checkpoint >= CHECKPOINT_INTERVAL {
            let _ = db.update_download_progress(db_id, downloaded as i64);
            last_checkpoint = downloaded;
        }

        // Progress SSE every 200ms
        if last_progress.elapsed() >= std::time::Duration::from_millis(200) {
            let elapsed = start.elapsed().as_secs_f64();
            let bytes_this_session = downloaded - actual_offset;
            let speed_kbps = if elapsed > 0.0 {
                (bytes_this_session as f64 / 1024.0) / elapsed
            } else {
                0.0
            };

            send_event(
                &tx,
                serde_json::json!({
                    "type": "progress",
                    "bytes": downloaded,
                    "total": total,
                    "speed_kbps": speed_kbps as u64,
                }),
            );
            last_progress = std::time::Instant::now();

            // Cancel if client disconnected — save progress but keep .part file
            if tx.is_closed() {
                if db_id > 0 {
                    let _ = db.update_download_progress(db_id, downloaded as i64);
                }
                return;
            }
        }
    }

    // 8. Done — rename .part to final file
    drop(file); // Close file handle before rename
    if let Err(e) = std::fs::rename(part_file, dest_file) {
        send_event(
            &tx,
            serde_json::json!({"type": "error", "message": format!("Cannot rename .part file: {e}")}),
        );
        return;
    }

    // Mark completed in DB
    if db_id > 0 {
        let _ = db.mark_download_completed(db_id, downloaded as i64);
    }

    send_event(
        &tx,
        serde_json::json!({
            "type": "done",
            "path": dest_file.to_string_lossy(),
            "bytes": downloaded,
        }),
    );
}

pub async fn handle_post_download(
    req: Request<Body>,
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    let dl: DownloadRequest = match parse_json_body(req.into_body()).await {
        Ok(r) => r,
        Err(e) => return Ok(e),
    };

    // Sanitize filename — strip any path components
    let sanitized = Path::new(&dl.filename)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("download.gguf")
        .to_string();

    let dest_dir = PathBuf::from(&dl.destination);
    if !dest_dir.is_dir() {
        return Ok(json_error(
            StatusCode::BAD_REQUEST,
            "Destination directory does not exist",
        ));
    }

    let dest_file = dest_dir.join(&sanitized);
    let part_file = dest_dir.join(format!("{sanitized}.part"));

    // If the final file already exists, return done immediately
    if dest_file.exists() {
        let size = std::fs::metadata(&dest_file).map(|m| m.len()).unwrap_or(0);
        let done = serde_json::json!({
            "type": "done",
            "path": dest_file.to_string_lossy(),
            "bytes": size,
        });
        let sse = format!("data: {done}\n\n");
        return Ok(Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/event-stream")
            .header("cache-control", "no-cache")
            .header("access-control-allow-origin", "*")
            .header("access-control-allow-methods", "GET, POST, PUT, DELETE, OPTIONS")
            .header("access-control-allow-headers", "content-type, authorization")
            .body(Body::from(sse))
            .unwrap());
    }

    // Build HF download URL
    let url = format!(
        "https://huggingface.co/{}/resolve/main/{}",
        dl.model_id,
        urlencoding::encode(&dl.filename),
    );

    // SSE channel
    let (mut sender, body) = Body::channel();
    let (progress_tx, mut progress_rx) = mpsc::channel::<String>(64);

    // Capture values for the blocking thread
    let model_id = dl.model_id.clone();
    let filename = dl.filename.clone();
    let dest_path_str = dl.destination.clone();
    let db_clone = db.clone();

    // Blocking download thread
    spawn_blocking(move || {
        download_file_blocking(
            &url,
            &dest_file,
            &part_file,
            db_clone,
            &model_id,
            &filename,
            &dest_path_str,
            progress_tx,
        );
    });

    // Forward progress events to SSE
    tokio::spawn(async move {
        while let Some(event) = progress_rx.recv().await {
            let sse = format!("data: {event}\n\n");
            if sender
                .send_data(hyper::body::Bytes::from(sse))
                .await
                .is_err()
            {
                break;
            }
        }
    });

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-cache")
        .header("access-control-allow-origin", "*")
        .header("access-control-allow-methods", "GET, POST, PUT, DELETE, OPTIONS")
        .header("access-control-allow-headers", "content-type, authorization")
        .header("connection", "keep-alive")
        .header("x-accel-buffering", "no")
        .body(body)
        .unwrap())
}

/// GET /api/hub/downloads — return all download records
pub async fn handle_get_downloads(
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    let records = db.get_hub_downloads().unwrap_or_default();
    let json = serde_json::to_string(&records).unwrap_or_else(|_| "[]".to_string());
    Ok(json_raw(StatusCode::OK, json))
}

/// POST /api/hub/downloads/verify — check which downloaded files still exist on disk,
/// delete missing records from DB, return the clean list.
/// For completed records: check final file. For pending records: check .part file.
pub async fn handle_post_verify(
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    let db2 = db.clone();
    let result = tokio::task::spawn_blocking(move || {
        let records = db2.get_hub_downloads().unwrap_or_default();
        let mut missing_ids = Vec::new();
        let mut valid = Vec::new();

        for rec in records {
            let sanitized_name = Path::new(&rec.filename)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&rec.filename);

            if rec.status == "completed" {
                // Check if final file exists
                let final_path = PathBuf::from(&rec.dest_path).join(sanitized_name);
                if final_path.exists() {
                    valid.push(rec);
                } else {
                    missing_ids.push(rec.id);
                }
            } else {
                // Pending: check if .part file exists
                let part_path =
                    PathBuf::from(&rec.dest_path).join(format!("{sanitized_name}.part"));
                if part_path.exists() {
                    valid.push(rec);
                } else {
                    missing_ids.push(rec.id);
                }
            }
        }

        if !missing_ids.is_empty() {
            let _ = db2.delete_hub_downloads_by_ids(&missing_ids);
        }

        valid
    })
    .await
    .unwrap_or_default();

    let json = serde_json::to_string(&result).unwrap_or_else(|_| "[]".to_string());
    Ok(json_raw(StatusCode::OK, json))
}
