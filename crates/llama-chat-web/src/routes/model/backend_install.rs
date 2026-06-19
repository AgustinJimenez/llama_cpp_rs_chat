#![allow(dead_code, unused_imports)]

use hyper::{Body, Response, StatusCode};
use std::convert::Infallible;
use std::io::Read;
use std::path::PathBuf;
use tokio::sync::mpsc;
use tokio::task::spawn_blocking;

use crate::response_helpers::json_error;

const GPU_BACKEND_RELEASE_URL: &str =
    "https://github.com/AgustinJimenez/llama_cpp_rs_chat/releases/download/backends/ggml-cuda.dll";

pub async fn handle_post_backends_install() -> Result<Response<Body>, Infallible> {
    let exe_dir = match std::env::current_exe() {
        Ok(exe) => exe
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .to_path_buf(),
        Err(e) => {
            return Ok(json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("Cannot determine app directory: {e}"),
            ));
        }
    };
    let dest_file = exe_dir.join("ggml-cuda.dll");

    if dest_file.exists() {
        let size = std::fs::metadata(&dest_file).map(|m| m.len()).unwrap_or(0);
        let done =
            serde_json::json!({ "type": "done", "path": dest_file.to_string_lossy(), "bytes": size });
        let sse = format!("data: {done}\n\n");
        return Ok(Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/event-stream")
            .header("cache-control", "no-cache")
            .header("access-control-allow-origin", "*")
            .body(Body::from(sse))
            .unwrap());
    }

    let (mut sender, body) = Body::channel();
    let (progress_tx, mut progress_rx) = mpsc::channel::<String>(64);
    spawn_blocking(move || download_backend_blocking(GPU_BACKEND_RELEASE_URL, &dest_file, progress_tx));

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
        .body(body)
        .unwrap())
}

fn download_backend_blocking(url: &str, dest: &PathBuf, tx: mpsc::Sender<String>) {
    use std::io::Write;

    let send = |json: serde_json::Value| {
        let _ = tx.blocking_send(json.to_string());
    };

    let agent = ureq::AgentBuilder::new()
        .redirects(10)
        .timeout(std::time::Duration::from_secs(600))
        .build();

    let resp = match agent.get(url).call() {
        Ok(r) => r,
        Err(e) => {
            send(serde_json::json!({ "type": "error", "message": format!("Download failed: {e}") }));
            return;
        }
    };

    let total = resp
        .header("content-length")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);
    let part_file = dest.with_extension("dll.part");
    let mut file = match std::fs::File::create(&part_file) {
        Ok(f) => f,
        Err(e) => {
            send(serde_json::json!({ "type": "error", "message": format!("Cannot create file: {e}") }));
            return;
        }
    };

    let mut reader = resp.into_reader();
    let mut buf = [0u8; 65536];
    let mut downloaded: u64 = 0;
    let mut last_report = std::time::Instant::now();

    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if file.write_all(&buf[..n]).is_err() {
                    send(serde_json::json!({ "type": "error", "message": "Write error" }));
                    return;
                }
                downloaded += n as u64;
                if last_report.elapsed().as_millis() >= 200 {
                    send(serde_json::json!({
                        "type": "progress",
                        "bytes": downloaded,
                        "total": total,
                        "percent": (downloaded * 100).checked_div(total).unwrap_or(0) as u32,
                    }));
                    last_report = std::time::Instant::now();
                }
            }
            Err(e) => {
                send(serde_json::json!({ "type": "error", "message": format!("Read error: {e}") }));
                return;
            }
        }
    }

    send(serde_json::json!({
        "type": "progress",
        "bytes": downloaded,
        "total": total,
        "percent": 100
    }));

    if let Err(e) = file.flush() {
        send(serde_json::json!({ "type": "error", "message": format!("Flush error: {e}") }));
        return;
    }
    if let Err(e) = std::fs::rename(&part_file, dest) {
        send(serde_json::json!({ "type": "error", "message": format!("Install error: {e}") }));
        return;
    }

    send(serde_json::json!({
        "type": "done",
        "path": dest.to_string_lossy(),
        "bytes": downloaded
    }));
}
