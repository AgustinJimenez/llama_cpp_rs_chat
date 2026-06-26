//! PTY-backed terminal WebSocket handler.
//!
//! Protocol:
//!   Client → Server:
//!     binary frames — raw stdin bytes (keyboard input, paste, etc.)
//!     text JSON     — `{"type":"resize","cols":N,"rows":N}`
//!   Server → Client:
//!     binary frames — raw stdout/stderr bytes with ANSI sequences (xterm renders them)
//!     text JSON     — `{"type":"exit"}` when the shell process exits

use std::io::{Read, Write};

use futures_util::{SinkExt, StreamExt};
use hyper::upgrade::Upgraded;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use tokio::sync::mpsc;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::protocol::Message as WsMessage;

const DEFAULT_COLS: u16 = 120;
const DEFAULT_ROWS: u16 = 30;

pub async fn handle_terminal_ws(
    upgraded: Upgraded,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let ws_stream = WebSocketStream::from_raw_socket(
        upgraded,
        tokio_tungstenite::tungstenite::protocol::Role::Server,
        None,
    )
    .await;

    let (mut ws_sender, mut ws_receiver) = ws_stream.split();

    let pty_system = native_pty_system();
    let pair = pty_system.openpty(PtySize {
        rows: DEFAULT_ROWS,
        cols: DEFAULT_COLS,
        pixel_width: 0,
        pixel_height: 0,
    })?;

    let cmd = default_shell();
    let mut child = pair.slave.spawn_command(cmd)?;
    drop(pair.slave);

    let mut pty_writer = pair.master.take_writer()?;
    let mut pty_reader = pair.master.try_clone_reader()?;

    let (pty_out_tx, mut pty_out_rx) = mpsc::channel::<Vec<u8>>(64);

    // Blocking read thread: PTY stdout → channel → WS binary frames
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match pty_reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if pty_out_tx.blocking_send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });

    loop {
        tokio::select! {
            // PTY output → WS
            chunk = pty_out_rx.recv() => {
                match chunk {
                    Some(bytes) => {
                        if ws_sender.send(WsMessage::Binary(bytes)).await.is_err() {
                            break;
                        }
                    }
                    None => {
                        let _ = ws_sender
                            .send(WsMessage::Text(r#"{"type":"exit"}"#.to_string()))
                            .await;
                        break;
                    }
                }
            }
            // WS → PTY stdin or control
            msg = ws_receiver.next() => {
                match msg {
                    Some(Ok(WsMessage::Binary(bytes))) => {
                        let _ = pty_writer.write_all(&bytes);
                        let _ = pty_writer.flush();
                    }
                    Some(Ok(WsMessage::Text(text))) => {
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                            if v.get("type").and_then(|t| t.as_str()) == Some("resize") {
                                let cols = v["cols"].as_u64().unwrap_or(u64::from(DEFAULT_COLS)) as u16;
                                let rows = v["rows"].as_u64().unwrap_or(u64::from(DEFAULT_ROWS)) as u16;
                                let _ = pair.master.resize(PtySize {
                                    rows,
                                    cols,
                                    pixel_width: 0,
                                    pixel_height: 0,
                                });
                            }
                        }
                    }
                    Some(Ok(WsMessage::Close(_))) | None => break,
                    Some(Ok(WsMessage::Ping(d))) => {
                        let _ = ws_sender.send(WsMessage::Pong(d)).await;
                    }
                    _ => {}
                }
            }
        }
    }

    let _ = child.kill();
    Ok(())
}

fn default_shell() -> CommandBuilder {
    #[cfg(windows)]
    {
        CommandBuilder::new("cmd.exe")
    }
    #[cfg(not(windows))]
    {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
        CommandBuilder::new(shell)
    }
}
