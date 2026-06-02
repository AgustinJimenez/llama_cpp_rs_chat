// System monitoring route handlers

use hyper::{Body, Request, Response, StatusCode};
use std::convert::Infallible;

use crate::response_helpers::{json_error, json_raw, json_response};
use llama_chat_db::SharedDatabase;

/// POST /api/browser/navigate — open/navigate the wry browser window (web mode).
pub async fn handle_browser_navigate(req: Request<Body>) -> Result<Response<Body>, Infallible> {
    let body = hyper::body::to_bytes(req.into_body()).await.unwrap_or_default();
    let url = serde_json::from_slice::<serde_json::Value>(&body)
        .ok()
        .and_then(|v| v.get("url").and_then(|u| u.as_str()).map(|s| s.to_string()));
    let Some(url) = url else {
        return Ok(json_response(
            StatusCode::BAD_REQUEST,
            &serde_json::json!({"success": false, "error": "url required"}),
        ));
    };
    match tokio::task::spawn_blocking(move || {
        llama_chat_tools::browser_session::notify_tauri_browser_navigate(&url)
    })
    .await
    {
        Ok(Ok(())) => Ok(json_response(
            StatusCode::OK,
            &serde_json::json!({"success": true}),
        )),
        Ok(Err(e)) => Ok(json_response(
            StatusCode::OK,
            &serde_json::json!({"success": false, "error": e}),
        )),
        Err(e) => Ok(json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &serde_json::json!({"success": false, "error": format!("task failed: {e}")}),
        )),
    }
}

/// POST /api/browser/close — close the wry browser window (web mode).
pub async fn handle_browser_close() -> Result<Response<Body>, Infallible> {
    let _ = tokio::task::spawn_blocking(|| {
        llama_chat_tools::browser_session::notify_tauri_browser_close()
    })
    .await;
    Ok(json_response(
        StatusCode::OK,
        &serde_json::json!({"success": true}),
    ))
}
#[cfg(target_os = "windows")]
use std::sync::Mutex;
#[cfg(target_os = "windows")]
use std::time::{Duration, Instant};
#[cfg(target_os = "windows")]
use tokio::task::spawn_blocking;
#[cfg(target_os = "windows")]
use tokio::time::timeout;

/// GET /api/info — app and system information
pub async fn handle_app_info() -> Result<Response<Body>, Infallible> {
    let info = serde_json::json!({
        "app": "llama-chat",
        "version": env!("CARGO_PKG_VERSION"),
        "platform": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "features": {
            "vision": cfg!(feature = "vision"),
            "cuda": cfg!(feature = "cuda"),
        },
    });
    Ok(json_raw(
        StatusCode::OK,
        serde_json::to_string(&info).unwrap(),
    ))
}

/// GET /api/docs — list all API endpoints
pub async fn handle_api_docs() -> Result<Response<Body>, Infallible> {
    let e = |method: &str, path: &str, desc: &str| -> serde_json::Value {
        serde_json::json!({"method": method, "path": path, "description": desc})
    };
    let endpoints = vec![
        e("GET", "/health", "Health check"),
        e("GET", "/api/info", "App and system info"),
        e("GET", "/api/docs", "This endpoint — API documentation"),
        e("POST", "/api/chat", "Send message (local model)"),
        e(
            "POST",
            "/api/chat/stream",
            "Send message with SSE streaming (local model)",
        ),
        e("POST", "/api/chat/cancel", "Cancel current generation"),
        e(
            "GET",
            "/api/conversations",
            "List all conversations (optional ?q=term to search)",
        ),
        e(
            "POST",
            "/api/conversations",
            "Create new conversation (optional worker_id)",
        ),
        e("GET", "/api/conversation/{id}", "Get conversation messages"),
        e("DELETE", "/api/conversations/{id}", "Delete a conversation"),
        e(
            "DELETE",
            "/api/conversations/batch",
            "Delete multiple conversations",
        ),
        e(
            "PATCH",
            "/api/conversations/{id}/title",
            "Rename a conversation",
        ),
        e(
            "PATCH",
            "/api/conversations/{id}/worker",
            "Bind conversation to worker (null/default clears to default)",
        ),
        e(
            "POST",
            "/api/conversations/{id}/truncate",
            "Truncate conversation at message",
        ),
        e(
            "GET",
            "/api/conversations/{id}/events",
            "Get conversation event log",
        ),
        e(
            "GET",
            "/api/conversations/{id}/metrics",
            "Get conversation metrics",
        ),
        e(
            "GET",
            "/api/conversations/{id}/token-analysis",
            "Token usage breakdown",
        ),
        e(
            "GET",
            "/api/conversation/{id}/export",
            "Export as markdown or JSON (?format=md|json)",
        ),
        e("GET", "/api/workers", "List active local workers"),
        e(
            "POST",
            "/api/workers",
            "Spawn new worker and load model (model_path + optional gpu_layers/mmproj_path)",
        ),
        e(
            "GET",
            "/api/workers/{id}/status",
            "Get per-worker model status",
        ),
        e(
            "DELETE",
            "/api/workers/{id}",
            "Remove worker and rebind its conversations to default",
        ),
        e("GET", "/api/model/status", "Current model status"),
        e(
            "GET",
            "/api/model/info",
            "Detailed model info (GGUF metadata)",
        ),
        e("POST", "/api/model/load", "Load a GGUF model"),
        e("POST", "/api/model/unload", "Unload current model"),
        e(
            "POST",
            "/api/model/hard-unload",
            "Force-kill worker to reclaim all VRAM",
        ),
        e("GET", "/api/model/history", "Recently used model paths"),
        e(
            "GET",
            "/api/providers",
            "List all providers with availability",
        ),
        e(
            "GET",
            "/api/providers/{id}/models",
            "Fetch available models from provider API",
        ),
        e(
            "POST",
            "/api/providers/{id}/generate",
            "Generate with cloud provider (blocking)",
        ),
        e(
            "POST",
            "/api/providers/{id}/stream",
            "Generate with cloud provider (SSE streaming)",
        ),
        e("GET", "/api/config", "Get sampler/app configuration"),
        e("POST", "/api/config", "Update configuration"),
        e(
            "GET",
            "/api/config/provider-keys",
            "Get configured provider API keys (masked)",
        ),
        e(
            "POST",
            "/api/config/provider-keys",
            "Set a provider API key",
        ),
        e(
            "GET",
            "/api/config/active-provider",
            "Get active provider and model",
        ),
        e(
            "POST",
            "/api/config/active-provider",
            "Set active provider and model",
        ),
        e(
            "GET",
            "/api/tools/available",
            "List available tools with schemas",
        ),
        e("POST", "/api/tools/execute", "Execute a tool call"),
        e("GET", "/api/tools/web-fetch", "Fetch web page as text"),
        e("GET", "/api/agents", "List all agents"),
        e("POST", "/api/agents", "Create an agent"),
        e("GET", "/api/agents/{id}", "Get agent by ID"),
        e("PUT", "/api/agents/{id}", "Update an agent"),
        e("DELETE", "/api/agents/{id}", "Delete an agent"),
        e(
            "GET",
            "/api/conversations/{id}/agent",
            "Get active agent for conversation",
        ),
        e(
            "POST",
            "/api/conversations/{id}/agent",
            "Assign agent to conversation",
        ),
        e("GET", "/api/mcp/servers", "List MCP servers"),
        e("POST", "/api/mcp/servers", "Add MCP server"),
        e("DELETE", "/api/mcp/servers/{id}", "Remove MCP server"),
        e(
            "POST",
            "/api/mcp/servers/{id}/toggle",
            "Enable/disable MCP server",
        ),
        e("POST", "/api/mcp/refresh", "Refresh MCP connections"),
        e("GET", "/api/mcp/tools", "List discovered MCP tools"),
        e("GET", "/api/system/usage", "CPU/memory/GPU usage"),
        e("GET", "/api/system/processes", "List background processes"),
        e(
            "POST",
            "/api/system/processes/kill",
            "Kill a background process",
        ),
        e(
            "POST",
            "/api/desktop/abort",
            "Abort current desktop automation",
        ),
        e("GET", "/api/browse", "Browse filesystem for model files"),
        e("POST", "/api/upload", "Upload a model file"),
        e("GET", "/api/hub/search", "Search HuggingFace Hub"),
        e("GET", "/api/hub/tree", "List files in a HuggingFace repo"),
        e("POST", "/api/hub/download", "Download model from Hub"),
        e("GET", "/api/hub/downloads", "List active downloads"),
        e("DELETE", "/api/hub/downloads", "Cancel a download"),
        e(
            "POST",
            "/api/hub/downloads/verify",
            "Verify downloaded file integrity",
        ),
    ];
    let docs = serde_json::json!({"endpoints": endpoints});
    Ok(json_raw(
        StatusCode::OK,
        serde_json::to_string_pretty(&docs).unwrap(),
    ))
}

/// POST /api/desktop/abort — abort current desktop automation
pub async fn handle_desktop_abort() -> Result<Response<Body>, Infallible> {
    llama_chat_desktop_tools::set_desktop_abort(true);
    Ok(json_raw(
        StatusCode::OK,
        serde_json::to_string(&serde_json::json!({
            "success": true,
            "message": "Desktop abort signal sent"
        }))
        .unwrap(),
    ))
}

pub async fn handle_system_usage() -> Result<Response<Body>, Infallible> {
    // Get system usage using Windows-native commands
    #[cfg(target_os = "windows")]
    let (cpu_usage, ram_usage, gpu_usage, cpu_perf_pct, app_ram_gb, vram_used_gb) = {
        let started = Instant::now();
        let result = timeout(
            Duration::from_millis(3000),
            spawn_blocking(get_windows_system_usage),
        )
        .await;

        match result {
            Ok(Ok(values)) => values,
            Ok(Err(join_err)) => {
                sys_warn!("[SYSTEM USAGE] spawn_blocking failed: {}", join_err);
                get_cached_windows_system_usage()
            }
            Err(_) => {
                sys_warn!(
                    "[SYSTEM USAGE] Timed out after {:?}, returning cached values",
                    started.elapsed()
                );
                get_cached_windows_system_usage()
            }
        }
    };

    #[cfg(not(target_os = "windows"))]
    let (cpu_usage, ram_usage, gpu_usage, _cpu_perf_pct, app_ram_gb, vram_used_gb) = (0.0_f32, 0.0_f32, 0.0_f32, 100.0_f32, 0.0_f32, 0.0_f32);

    // Get hardware totals (cached alongside usage)
    #[cfg(target_os = "windows")]
    let (total_ram_gb, total_vram_gb, cpu_cores, cpu_base_mhz) = get_hardware_totals();
    #[cfg(target_os = "macos")]
    let (total_ram_gb, total_vram_gb, cpu_cores, _cpu_base_mhz) = {
        let ram = silent_command("sysctl")
            .args(["-n", "hw.memsize"])
            .output()
            .ok()
            .and_then(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .trim()
                    .parse::<u64>()
                    .ok()
            })
            .map(|b| b as f32 / 1_073_741_824.0)
            .unwrap_or(0.0);
        let cores = silent_command("sysctl")
            .args(["-n", "hw.ncpu"])
            .output()
            .ok()
            .and_then(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .trim()
                    .parse::<u32>()
                    .ok()
            })
            .unwrap_or(0);
        // macOS unified memory — GPU shares RAM
        (ram, ram, cores, 0_u32)
    };
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let (total_ram_gb, total_vram_gb, cpu_cores, _cpu_base_mhz) = (0.0_f32, 0.0_f32, 0_u32, 0_u32);

    // Current CPU speed = base_mhz * (perf% / 100)
    #[cfg(target_os = "windows")]
    let cpu_ghz = (cpu_base_mhz as f32) * cpu_perf_pct / 100.0 / 1000.0;
    #[cfg(not(target_os = "windows"))]
    let cpu_ghz = 0.0_f32;

    let response = serde_json::json!({
        "cpu": cpu_usage,
        "gpu": gpu_usage,
        "ram": ram_usage,
        "total_ram_gb": total_ram_gb,
        "total_vram_gb": total_vram_gb,
        "cpu_cores": cpu_cores,
        "cpu_ghz": cpu_ghz,
        "app_ram_gb": app_ram_gb,
        "vram_used_gb": vram_used_gb,
    });

    Ok(json_raw(
        StatusCode::OK,
        serde_json::to_string(&response).unwrap(),
    ))
}

#[cfg(target_os = "windows")]
lazy_static::lazy_static! {
    /// Cached usage: (timestamp, cpu%, ram%, gpu%, cpu_perf_pct, app_ram_gb, vram_used_gb)
    static ref LAST_USAGE: Mutex<(Instant, f32, f32, f32, f32, f32, f32)> =
        Mutex::new((Instant::now(), 0.0, 0.0, 0.0, 100.0, 0.0, 0.0));
    /// Cached hardware totals: (total_ram_gb, total_vram_gb, cpu_cores, cpu_base_mhz)
    static ref HARDWARE_TOTALS: Mutex<(f32, f32, u32, u32)> = Mutex::new((0.0, 0.0, 0, 0));
}

#[cfg(target_os = "windows")]
pub fn get_cached_windows_system_usage() -> (f32, f32, f32, f32, f32, f32) {
    let last = LAST_USAGE.lock().unwrap();
    (last.1, last.2, last.3, last.4, last.5, last.6)
}

use llama_chat_engine::utils::silent_command;

#[cfg(target_os = "windows")]
pub fn get_windows_system_usage() -> (f32, f32, f32, f32, f32, f32) {
    // Cache for 500ms to allow smooth real-time updates
    let mut last = LAST_USAGE.lock().unwrap();
    if last.0.elapsed() < Duration::from_millis(500) {
        return (last.1, last.2, last.3, last.4, last.5, last.6);
    }

    // Get CPU + RAM + app process RAM via CIM (instant — no Get-Counter 1 s sample wait).
    // Force InvariantCulture so numeric output always uses a period as the
    // decimal separator regardless of the system locale (e.g. Spanish uses comma).
    // Output: line 0 = cpu%, line 1 = perf% (fixed 100), line 2 = ram%, line 3 = app_ram_gb
    let app_pid = std::process::id();
    let ps_command = format!(
        "[System.Threading.Thread]::CurrentThread.CurrentCulture=[System.Globalization.CultureInfo]::InvariantCulture; \
        $cpu=(Get-CimInstance Win32_PerfFormattedData_PerfOS_Processor -Filter \"Name='_Total'\").PercentProcessorTime; \
        $os=Get-CimInstance Win32_OperatingSystem; \
        $ram=[math]::Round((($os.TotalVisibleMemorySize-$os.FreePhysicalMemory)/$os.TotalVisibleMemorySize)*100,2); \
        $appRam=[math]::Round((Get-Process -Id {app_pid} -ErrorAction SilentlyContinue).WorkingSet64/1073741824,3); \
        $cpu; 100; $ram; $appRam"
    );
    let cpu_ram_output = silent_command("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &ps_command,
        ])
        .output();

    let (cpu_usage, cpu_perf_pct, ram_usage, app_ram_gb) = if let Ok(output) = cpu_ram_output {
        if !output.status.success() {
            sys_debug!(
                "[SYSTEM USAGE] CPU/RAM command failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        let lines: Vec<f32> = String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|l| l.trim().parse::<f32>().ok())
            .collect();
        (
            lines.first().copied().unwrap_or(0.0),
            lines.get(1).copied().unwrap_or(100.0),
            lines.get(2).copied().unwrap_or(0.0),
            lines.get(3).copied().unwrap_or(0.0),
        )
    } else {
        (0.0, 100.0, 0.0, 0.0)
    };

    // Get GPU usage + VRAM used via nvidia-smi (if available)
    // Queries both utilization% and memory.used in one call to avoid two process spawns.
    let gpu_output = silent_command("nvidia-smi")
        .args([
            "--query-gpu=utilization.gpu,memory.used",
            "--format=csv,noheader,nounits",
        ])
        .output();

    let (gpu_usage, vram_used_gb) = if let Ok(output) = gpu_output {
        if !output.status.success() {
            sys_debug!(
                "[SYSTEM USAGE] GPU command failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        let line = String::from_utf8_lossy(&output.stdout);
        let mut parts = line.lines().next().unwrap_or("").split(',');
        let gpu_pct = parts.next().and_then(|s| s.trim().parse::<f32>().ok()).unwrap_or(0.0);
        let vram_mb = parts.next().and_then(|s| s.trim().parse::<f32>().ok()).unwrap_or(0.0);
        (gpu_pct, vram_mb / 1024.0)
    } else {
        (0.0, 0.0)
    };

    // Detect hardware totals (only once, when still at defaults)
    {
        let mut hw = HARDWARE_TOTALS.lock().unwrap();
        if hw.0 == 0.0 {
            // Total RAM via CIM (returns KB)
            if let Ok(output) = silent_command("powershell")
                .args([
                    "-NoProfile",
                    "-NonInteractive",
                    "-Command",
                    "(Get-CimInstance Win32_OperatingSystem).TotalVisibleMemorySize",
                ])
                .output()
            {
                if let Ok(kb) = String::from_utf8_lossy(&output.stdout)
                    .trim()
                    .parse::<f64>()
                {
                    hw.0 = (kb / 1_048_576.0) as f32; // KB → GB
                }
            }
            // CPU logical processors + base clock
            if let Ok(output) = silent_command("powershell")
                .args(["-NoProfile", "-NonInteractive", "-Command",
                    "Get-CimInstance Win32_Processor | ForEach-Object { $_.NumberOfLogicalProcessors; $_.MaxClockSpeed }"])
                .output()
            {
                let lines: Vec<String> = String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .map(|l| l.trim().to_string())
                    .filter(|l| !l.is_empty())
                    .collect();
                if let Some(cores) = lines.first().and_then(|s| s.parse::<u32>().ok()) {
                    hw.2 = cores;
                }
                if let Some(mhz) = lines.get(1).and_then(|s| s.parse::<u32>().ok()) {
                    hw.3 = mhz;
                }
            }
            // Total VRAM via nvidia-smi
            if let Ok(output) = silent_command("nvidia-smi")
                .args(["--query-gpu=memory.total", "--format=csv,noheader,nounits"])
                .output()
            {
                if let Some(mb) = String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .next()
                    .and_then(|l| l.trim().parse::<f64>().ok())
                {
                    hw.1 = (mb / 1024.0) as f32; // MB → GB
                }
            }
            sys_debug!(
                "[SYSTEM] Detected hardware: {:.1} GB RAM, {:.1} GB VRAM",
                hw.0,
                hw.1
            );
        }
    }

    // Update cache
    *last = (
        Instant::now(),
        cpu_usage,
        ram_usage,
        gpu_usage,
        cpu_perf_pct,
        app_ram_gb,
        vram_used_gb,
    );

    (cpu_usage, ram_usage, gpu_usage, cpu_perf_pct, app_ram_gb, vram_used_gb)
}

#[cfg(not(target_os = "windows"))]
#[allow(dead_code)]
pub fn get_windows_system_usage() -> (f32, f32, f32, f32, f32, f32) {
    // Return placeholder values on non-Windows platforms
    (0.0, 0.0, 0.0, 100.0, 0.0, 0.0)
}

/// Returns cached hardware totals: (total_ram_gb, total_vram_gb, cpu_cores, cpu_base_mhz).
/// On Windows these are populated lazily by `get_windows_system_usage`. On other
/// platforms they're zeros — the Tauri command falls back to its own detection.
#[cfg(target_os = "windows")]
pub fn get_hardware_totals() -> (f32, f32, u32, u32) {
    let hw = HARDWARE_TOTALS.lock().unwrap();
    (hw.0, hw.1, hw.2, hw.3)
}

// ── Background process endpoints ────────────────────────────────────────────

pub async fn handle_background_processes(db: SharedDatabase) -> Result<Response<Body>, Infallible> {
    let result = tokio::task::spawn_blocking(move || {
        let conn = db.connection();

        // Query all background processes from DB
        let mut stmt = match conn.prepare(
            "SELECT pid, command, conversation_id, started_at, session_id FROM background_processes"
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };

        let rows: Vec<(i64, String, Option<String>, i64, String)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })
            .ok()
            .map(|iter| iter.filter_map(|r| r.ok()).collect())
            .unwrap_or_default();

        let mut processes = Vec::new();
        let mut dead_pids = Vec::new();

        for (pid, command, conversation_id, started_at, _session_id) in rows {
            let alive = llama_chat_command::background::is_process_alive(pid as u32);
            if !alive {
                dead_pids.push(pid);
            }
            processes.push(serde_json::json!({
                "pid": pid,
                "command": command,
                "conversationId": conversation_id,
                "startedAt": started_at,
                "alive": alive,
            }));
        }

        // Clean up dead records
        for pid in dead_pids {
            let _ = conn.execute("DELETE FROM background_processes WHERE pid = ?1", [pid]);
        }

        // Only return alive processes
        processes.retain(|p| p["alive"].as_bool().unwrap_or(false));
        processes
    })
    .await
    .unwrap_or_default();

    Ok(json_response(StatusCode::OK, &result))
}

pub async fn handle_kill_process(
    req: Request<Body>,
    db: SharedDatabase,
) -> Result<Response<Body>, Infallible> {
    let body = match hyper::body::to_bytes(req.into_body()).await {
        Ok(b) => b,
        Err(_) => return Ok(json_error(StatusCode::BAD_REQUEST, "Failed to read body")),
    };

    let parsed: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return Ok(json_error(StatusCode::BAD_REQUEST, "Invalid JSON")),
    };

    let pid = match parsed["pid"].as_u64() {
        Some(p) => p as u32,
        None => return Ok(json_error(StatusCode::BAD_REQUEST, "Missing pid")),
    };

    let result = tokio::task::spawn_blocking(move || {
        // Kill the process tree
        llama_chat_command::background::kill_background_process_by_pid(pid);

        // Wait briefly for process to die, then verify
        std::thread::sleep(std::time::Duration::from_millis(500));
        let still_alive = llama_chat_command::background::is_process_alive(pid);

        // Remove from DB regardless (stale entries get cleaned on next list)
        let conn = db.connection();
        let _ = conn.execute(
            "DELETE FROM background_processes WHERE pid = ?1",
            [pid as i64],
        );

        !still_alive
    })
    .await
    .unwrap_or(false);

    if result {
        Ok(json_response(
            StatusCode::OK,
            &serde_json::json!({"success": true, "message": "Process killed"}),
        ))
    } else {
        Ok(json_response(
            StatusCode::OK,
            &serde_json::json!({"success": false, "message": "Process may not have been killed. It might require elevated permissions."}),
        ))
    }
}
