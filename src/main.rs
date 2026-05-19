// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[allow(unused_imports)]
#[macro_use]
extern crate llama_chat_types;

#[allow(dead_code)]
mod web;
mod commands;
mod event_payloads;

/// Enable Chrome DevTools Protocol on the WebView2 for remote debugging/automation.
/// Connect via http://localhost:9222 with CDP-compatible tools.
fn enable_cdp_debugging() {
    if std::env::var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS").is_err() {
        std::env::set_var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", "--remote-debugging-port=9222");
    }
}

use std::sync::Arc;

use chrono::Local;
use log::LevelFilter;
use log4rs::append::file::FileAppender;
use log4rs::config::{Appender, Config, Root};
use log4rs::encode::pattern::PatternEncoder;
use tauri::{Emitter, Manager, WindowEvent};
// WebviewBuilder, LogicalPosition, LogicalSize, WebviewUrl moved to commands::browser_panel

mod mcp_ui;
use tauri::menu::{MenuBuilder, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder};
use tauri::tray::TrayIconBuilder;

use web::database::{Database, SharedDatabase};
use web::worker::process_manager::ProcessManager;
use web::worker::worker_bridge::{SharedWorkerBridge, WorkerBridge};

// ─── Setup ────────────────────────────────────────────────────────────

fn setup_logging() -> Result<(), Box<dyn std::error::Error>> {
    let base = std::env::var("LLAMA_CHAT_DATA_DIR").unwrap_or_else(|_| ".".to_string());
    let log_dir = format!("{base}/logs");
    std::fs::create_dir_all(&log_dir)?;
    let timestamp = Local::now().format("%Y-%m-%d-%H_%M").to_string();
    let log_path = format!("{log_dir}/{timestamp}.log");

    let file_appender = FileAppender::builder()
        .encoder(Box::new(PatternEncoder::new(
            "{d(%Y-%m-%d %H:%M:%S)} - {l} - {m}{n}",
        )))
        .build(log_path)?;

    let config = Config::builder()
        .appender(Appender::builder().build("file", Box::new(file_appender)))
        .build(Root::builder().appender("file").build(LevelFilter::Info))?;

    log4rs::init_config(config)?;

    Ok(())
}

fn main() {
    // Enable CDP remote debugging (port 9222) for automation tools
    enable_cdp_debugging();

    // Check for --worker flag BEFORE Tauri setup.
    // The worker creates its own runtimes internally,
    // so it must not run inside an existing tokio runtime.
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--worker") {
        let db_path = args
            .windows(2)
            .find(|w| w[0] == "--db-path")
            .map(|w| w[1].as_str())
            .unwrap_or("assets/llama_chat.db");
        web::worker::worker_main::run_worker(db_path);
        return;
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(
                    tauri_plugin_window_state::StateFlags::POSITION
                        | tauri_plugin_window_state::StateFlags::SIZE
                        | tauri_plugin_window_state::StateFlags::MAXIMIZED
                        | tauri_plugin_window_state::StateFlags::VISIBLE,
                )
                .build(),
        )
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
            // Forward deep link URLs from second instance
            for arg in &args {
                if arg.starts_with("llamachat://") {
                    let _ = app.emit("deep-link", arg.clone());
                }
            }
        }))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_deep_link::init())
        .setup(|app| {
            // Resolve app data directory for persistent storage
            let data_dir = app.path().app_data_dir()
                .expect("Failed to resolve app data directory");
            std::fs::create_dir_all(&data_dir)
                .expect("Failed to create app data directory");
            let data_dir_str = data_dir.to_string_lossy().to_string();

            // Set env var so worker process and log modules use the same directory
            std::env::set_var("LLAMA_CHAT_DATA_DIR", &data_dir_str);
            eprintln!("[TAURI] Data directory: {data_dir_str}");

            // Initialize logging (after data dir is set)
            if let Err(e) = setup_logging() {
                eprintln!("Failed to set up logging: {e}");
            }

            let db_path = data_dir.join("llama_chat.db");
            let db_path_str = db_path.to_string_lossy().to_string();

            // Initialize SQLite database
            let db: SharedDatabase = Arc::new(
                Database::new(&db_path_str)
                    .expect("Failed to initialize SQLite database"),
            );
            eprintln!("[TAURI] Database initialized at {db_path_str}");

            // Initialize background process tracking for remote provider tool calls
            let bg_session_id = format!("tauri_{}", std::process::id());
            llama_chat_command::background::init_background_tracking(db.clone(), bg_session_id);

            // Spawn worker process
            let pm = Arc::new(
                ProcessManager::spawn(&db_path_str)
                    .expect("Failed to spawn worker process"),
            );
            let bridge: SharedWorkerBridge = Arc::new(
                tauri::async_runtime::block_on(async { WorkerBridge::new(pm) }),
            );
            eprintln!("[TAURI] Worker process spawned, bridge ready");

            // Register managed state
            app.manage(db);
            app.manage(bridge);

            // MCP UI server — direct WebView2 ExecuteScript (no HTTP callbacks needed)
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                mcp_ui::start_default(app_handle).await;
            });

            // ─── App Menu ────────────────────────────────────────────
            let new_chat = MenuItemBuilder::with_id("new-chat", "New Chat")
                .accelerator("CmdOrCtrl+N")
                .build(app)?;
            let settings = MenuItemBuilder::with_id("open-settings", "Settings...")
                .accelerator("CmdOrCtrl+,")
                .build(app)?;
            let quit = MenuItemBuilder::with_id("quit", "Quit")
                .accelerator("CmdOrCtrl+Q")
                .build(app)?;

            let file_menu = SubmenuBuilder::new(app, "File")
                .item(&new_chat)
                .separator()
                .item(&settings)
                .separator()
                .item(&quit)
                .build()?;

            let edit_menu = SubmenuBuilder::new(app, "Edit")
                .item(&PredefinedMenuItem::undo(app, None)?)
                .item(&PredefinedMenuItem::redo(app, None)?)
                .separator()
                .item(&PredefinedMenuItem::cut(app, None)?)
                .item(&PredefinedMenuItem::copy(app, None)?)
                .item(&PredefinedMenuItem::paste(app, None)?)
                .item(&PredefinedMenuItem::select_all(app, None)?)
                .build()?;

            let menu = MenuBuilder::new(app)
                .item(&file_menu)
                .item(&edit_menu)
                .build()?;

            app.set_menu(menu)?;

            // ─── System Tray ─────────────────────────────────────────
            let tray_show = MenuItemBuilder::with_id("tray-show", "Show Window").build(app)?;
            let tray_new = MenuItemBuilder::with_id("tray-new-chat", "New Chat").build(app)?;
            let tray_quit = MenuItemBuilder::with_id("tray-quit", "Quit").build(app)?;

            let tray_menu = MenuBuilder::new(app)
                .item(&tray_show)
                .item(&tray_new)
                .separator()
                .item(&tray_quit)
                .build()?;

            TrayIconBuilder::new()
                .menu(&tray_menu)
                .tooltip("LLaMA Chat")
                .on_menu_event(|app, event| {
                    match event.id().as_ref() {
                        "tray-show" => {
                            if let Some(w) = app.get_webview_window("main") {
                                let _ = w.show();
                                let _ = w.set_focus();
                            }
                        }
                        "tray-new-chat" => {
                            if let Some(w) = app.get_webview_window("main") {
                                let _ = w.show();
                                let _ = w.set_focus();
                            }
                            let _ = app.emit("new-chat", ());
                        }
                        "tray-quit" => {
                            app.exit(0);
                        }
                        _ => {}
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    if let tauri::tray::TrayIconEvent::Click {
                        button: tauri::tray::MouseButton::Left,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                })
                .build(app)?;

            eprintln!("[TAURI] Menu and tray icon initialized");

            Ok(())
        })
        // ─── Menu Event Handler ──────────────────────────────────────
        .on_menu_event(|app, event| {
            match event.id().as_ref() {
                "new-chat" => {
                    let _ = app.emit("new-chat", ());
                }
                "open-settings" => {
                    let _ = app.emit("open-settings", ());
                }
                "quit" => {
                    app.exit(0);
                }
                _ => {}
            }
        })
        // ─── Window Close → Hide to Tray ─────────────────────────────
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                // Hide to tray instead of closing
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::logging::log_to_file,
            commands::logging::record_app_error,
            commands::logging::get_app_errors,
            commands::logging::clear_app_errors,
            // Configuration
            commands::config::get_config,
            commands::config::save_config,
            // Model
            commands::model::get_model_status,
            commands::model::load_model,
            commands::model::unload_model,
            commands::model::hard_unload,
            commands::model::get_model_info,
            commands::model::get_model_history,
            commands::model::add_model_history,
            // Conversations
            commands::conversation::get_conversations,
            commands::conversation::get_conversation,
            commands::conversation::delete_conversation,
            commands::conversation::truncate_conversation,
            commands::conversation::compact_conversation,
            commands::conversation::get_conversation_metrics,
            // Chat
            commands::chat::generate_stream,
            commands::chat::cancel_generation,
            // Files
            commands::files::browse_files,
            // Tools
            commands::tools::execute_tool,
            commands::tools::web_fetch,
            // System
            commands::providers::get_system_usage,
            // Native browser panel
            commands::browser_panel::browser_panel_open,
            commands::browser_panel::browser_panel_navigate,
            commands::browser_panel::browser_panel_get_info,
            commands::browser_panel::browser_panel_zoom,
            commands::browser_panel::browser_panel_set_zoom,
            commands::browser_panel::browser_panel_eval_js,
            commands::browser_panel::browser_panel_reload,
            commands::browser_panel::browser_panel_go_back,
            commands::browser_panel::browser_panel_go_forward,
            commands::browser_panel::browser_panel_resize,
            commands::browser_panel::browser_panel_close,
            // Agent browser API (external control)
            commands::browser_panel::agent_browser_navigate,
            commands::browser_panel::agent_browser_get_text,
            commands::browser_panel::agent_browser_get_links,
            commands::browser_panel::agent_browser_get_html,
            commands::browser_panel::agent_browser_click,
            commands::browser_panel::agent_browser_type_text,
            commands::browser_panel::agent_browser_eval,
            commands::browser_panel::agent_browser_search,
            commands::browser_panel::agent_browser_scroll,
            commands::browser_panel::agent_browser_query,
            // Providers
            commands::providers::list_providers,
            commands::providers::list_configured_providers,
            commands::providers::list_cli_providers,
            commands::providers::stream_provider,
            commands::providers::queue_message,
            // HuggingFace Hub
            commands::hub::search_hub_models,
            commands::hub::fetch_hub_tree,
            commands::hub::verify_hub_downloads,
            commands::hub::delete_hub_download,
            commands::hub::download_hub_model,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
