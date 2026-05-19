//! Status WebSocket handler — persistent keepalive for server crash detection.

use futures_util::{SinkExt, StreamExt};
use hyper::upgrade::Upgraded;
use tokio::time::Duration;
use tokio_tungstenite::tungstenite::protocol::Message as WsMessage;
use tokio_tungstenite::WebSocketStream;

use llama_chat_worker::worker::worker_bridge::SharedWorkerBridge;

use super::ACTIVE_WS_CONNECTIONS;
use std::sync::atomic::Ordering;

/// Persistent status WebSocket — keeps alive with pings, sends initial model
/// status, and lets the frontend detect server crashes via TCP close.
pub async fn handle_status_ws(
    upgraded: Upgraded,
    bridge: SharedWorkerBridge,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let ws_stream = WebSocketStream::from_raw_socket(
        upgraded,
        tokio_tungstenite::tungstenite::protocol::Role::Server,
        None,
    )
    .await;

    let (mut ws_sender, mut ws_receiver) = ws_stream.split();

    let _ = ACTIVE_WS_CONNECTIONS.fetch_add(1, Ordering::SeqCst);
    sys_info!("[WS_STATUS] Status WebSocket connected");

    // Send initial model status
    let loaded = bridge.model_status().await.is_some();
    let init_msg = serde_json::json!({
        "type": "model_status",
        "loaded": loaded
    });
    if ws_sender
        .send(WsMessage::Text(init_msg.to_string()))
        .await
        .is_err()
    {
        let _ = ACTIVE_WS_CONNECTIONS.fetch_sub(1, Ordering::SeqCst);
        return Ok(());
    }

    // Keep-alive loop: ping every 20s, listen for client messages
    let mut ping_interval = tokio::time::interval(Duration::from_secs(20));
    ping_interval.tick().await; // consume the immediate first tick

    loop {
        tokio::select! {
            _ = ping_interval.tick() => {
                if ws_sender.send(WsMessage::Ping(vec![])).await.is_err() {
                    break;
                }
            }
            ws_msg = ws_receiver.next() => {
                match ws_msg {
                    Some(Ok(WsMessage::Close(_))) | None => {
                        break;
                    }
                    Some(Ok(WsMessage::Ping(data))) => {
                        let _ = ws_sender.send(WsMessage::Pong(data)).await;
                    }
                    Some(Ok(WsMessage::Pong(_))) => {
                        // Expected response to our pings — ignore
                    }
                    Some(Err(_)) => {
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    let _ = ACTIVE_WS_CONNECTIONS.fetch_sub(1, Ordering::SeqCst);
    sys_info!("[WS_STATUS] Status WebSocket disconnected");
    Ok(())
}
