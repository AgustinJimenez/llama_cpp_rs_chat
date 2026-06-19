//! Miscellaneous command handlers: MCP, events, status, compaction, ping, title.

use std::io::Write;
use std::sync::Arc;

use llama_chat_db::SharedDatabase;
use llama_chat_types::models::SharedLlamaState;

use super::super::ipc_types::*;
use super::stdout::write_response;
use crate::mcp::McpManager;

/// Handle RefreshMcpServers command.
pub fn handle_refresh_mcp_servers(
    req_id: u64,
    mcp_manager: &McpManager,
    db: &SharedDatabase,
    ipc_writer: &mut impl Write,
) {
    eprintln!("[WORKER] Refreshing MCP server connections...");
    match mcp_manager.refresh_connections(db) {
        Ok(()) => {
            let tool_defs = mcp_manager.get_tool_definitions();
            let connected: Vec<String> = mcp_manager.get_connected_server_names();
            eprintln!("[WORKER] MCP refresh complete: {} servers, {} tools", connected.len(), tool_defs.len());
            write_response(ipc_writer, &WorkerResponse::ok(req_id, WorkerPayload::McpServersRefreshed {
                connected_servers: connected,
                total_tools: tool_defs.len(),
            }));
        }
        Err(e) => {
            eprintln!("[WORKER] MCP refresh failed: {e}");
            write_response(ipc_writer, &WorkerResponse::error(req_id, e));
        }
    }
}

/// Handle GetMcpStatus command.
pub fn handle_get_mcp_status(
    req_id: u64,
    mcp_manager: &McpManager,
    ipc_writer: &mut impl Write,
) {
    let statuses = mcp_manager.get_server_statuses();
    write_response(ipc_writer, &WorkerResponse::ok(req_id, WorkerPayload::McpStatus {
        servers: statuses,
    }));
}

/// Handle GetConversationEvents command.
pub fn handle_get_conversation_events(
    req_id: u64,
    conversation_id: &str,
    ipc_writer: &mut impl Write,
) {
    let events = llama_chat_db::event_log::get_events(conversation_id);
    write_response(ipc_writer, &WorkerResponse::ok(req_id, WorkerPayload::ConversationEvents { events }));
}

/// Handle GetGlobalStatus command.
pub fn handle_get_global_status(
    req_id: u64,
    ipc_writer: &mut impl Write,
) {
    let status = llama_chat_db::event_log::get_global_status();
    write_response(ipc_writer, &WorkerResponse::ok(req_id, WorkerPayload::GlobalStatus { status }));
}

/// Handle GetAvailableBackends command.
pub fn handle_get_available_backends(
    req_id: u64,
    ipc_writer: &mut impl Write,
) {
    #[cfg(feature = "dynamic-backends")]
    {
        let _backend = llama_cpp_2::llama_backend::LlamaBackend::init();
        if let Ok(ref b) = _backend { b.load_all_backends(); }
    }
    let devices = llama_cpp_2::list_llama_ggml_backend_devices();
    let mut backend_map: std::collections::HashMap<String, Vec<BackendDeviceInfo>> = std::collections::HashMap::new();
    for dev in &devices {
        let vram_mb = if dev.memory_total > 0 {
            Some((dev.memory_total / (1024 * 1024)) as u64)
        } else {
            None
        };
        backend_map.entry(dev.backend.clone()).or_default().push(
            BackendDeviceInfo {
                name: dev.name.clone(),
                description: dev.description.clone(),
                vram_mb,
            },
        );
    }
    let backends: Vec<BackendInfo> = backend_map
        .into_iter()
        .map(|(name, devices)| BackendInfo {
            available: true,
            name,
            devices,
        })
        .collect();
    write_response(ipc_writer, &WorkerResponse::ok(req_id, WorkerPayload::AvailableBackends { backends }));
}

/// Handle CompactConversation command. Spawns a status relay thread.
pub fn handle_compact_conversation(
    req_id: u64,
    conversation_id: String,
    llama_state: SharedLlamaState,
    db: &SharedDatabase,
    ipc_for_status: Arc<std::sync::Mutex<std::fs::File>>,
    ipc_writer: &mut impl Write,
) {
    eprintln!("[WORKER] Manual compaction requested for conv={conversation_id}");

    let (status_tx, mut status_rx) = tokio::sync::mpsc::unbounded_channel::<llama_chat_types::models::TokenData>();
    let ipc_status = ipc_for_status.clone();
    let status_thread = std::thread::spawn(move || {
        while let Some(data) = status_rx.blocking_recv() {
            if let Some(msg) = data.status {
                if let Ok(json) = serde_json::to_string(&WorkerResponse::ok(0, WorkerPayload::StatusUpdate { message: msg })) {
                    if let Ok(mut f) = ipc_status.lock() {
                        let _ = writeln!(f, "{json}");
                        let _ = f.flush();
                    }
                }
            }
        }
    });

    match llama_chat_engine::compaction::force_compact_conversation(&conversation_id, db, &llama_state, Some(&status_tx)) {
        Ok(()) => {
            eprintln!("[WORKER] Manual compaction complete for conv={conversation_id}");
            drop(status_tx);
            let _ = status_thread.join();
            write_response(ipc_writer, &WorkerResponse::ok(req_id, WorkerPayload::CompactionDone { conversation_id }));
        }
        Err(e) => {
            eprintln!("[WORKER] Manual compaction failed: {e}");
            drop(status_tx);
            let _ = status_thread.join();
            write_response(ipc_writer, &WorkerResponse::error(req_id, e));
        }
    }
}

/// Handle GetMcpToolDefinitions command.
pub fn handle_get_mcp_tool_definitions(
    req_id: u64,
    mcp_manager: &McpManager,
    ipc_writer: &mut impl Write,
) {
    let tools: Vec<llama_chat_types::ipc_types::McpToolDefPayload> = mcp_manager.get_tool_definitions()
        .into_iter()
        .map(|td| llama_chat_types::ipc_types::McpToolDefPayload {
            qualified_name: td.qualified_name,
            description: td.description,
            input_schema: td.input_schema,
            server_name: td.server_name,
        })
        .collect();
    write_response(ipc_writer, &WorkerResponse::ok(req_id, WorkerPayload::McpToolDefinitions { tools }));
}

/// Handle CallMcpTool command (called by the server when a remote provider needs to run an MCP tool).
pub fn handle_call_mcp_tool(
    req_id: u64,
    name: &str,
    args_json: &str,
    mcp_manager: &McpManager,
    ipc_writer: &mut impl Write,
) {
    let args: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => {
            write_response(ipc_writer, &WorkerResponse::ok(req_id, WorkerPayload::McpToolResult {
                result: None,
                error: Some(format!("Invalid args JSON: {e}")),
            }));
            return;
        }
    };
    match mcp_manager.call_tool(name, args) {
        Ok(result) => write_response(ipc_writer, &WorkerResponse::ok(req_id, WorkerPayload::McpToolResult {
            result: Some(result),
            error: None,
        })),
        Err(e) => write_response(ipc_writer, &WorkerResponse::ok(req_id, WorkerPayload::McpToolResult {
            result: None,
            error: Some(e),
        })),
    }
}

/// Handle GenerateTitle command.
pub fn handle_generate_title(
    req_id: u64,
    conversation_id: String,
    prompt: String,
    llama_state: SharedLlamaState,
    ipc_writer: &mut impl Write,
) {
    eprintln!("[WORKER] Generating title for conv={conversation_id}");
    match llama_chat_engine::generate_title_text(&llama_state, &prompt) {
        Ok(title) => {
            write_response(ipc_writer, &WorkerResponse::ok(
                req_id,
                WorkerPayload::TitleGenerated { conversation_id, title },
            ));
        }
        Err(e) => {
            eprintln!("[WORKER] Title generation failed: {e}");
            write_response(ipc_writer, &WorkerResponse::error(req_id, e));
        }
    }
}
