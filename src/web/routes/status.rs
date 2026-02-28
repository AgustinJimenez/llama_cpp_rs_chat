use std::convert::Infallible;

use hyper::{Body, Request, Response, StatusCode};

use crate::web::websocket_utils::{
    build_json_error_response, build_websocket_upgrade_response, calculate_websocket_accept_key,
    get_websocket_key, is_websocket_upgrade,
};

#[cfg(not(feature = "mock"))]
use crate::web::websocket::handle_status_ws;
#[cfg(not(feature = "mock"))]
use crate::web::worker::worker_bridge::SharedWorkerBridge;
#[cfg(not(feature = "mock"))]
use crate::sys_error;

/// WebSocket upgrade handler for persistent status/health connection.
///
/// The client opens this once on page load. The server sends an initial
/// model status message and then keeps the connection alive with pings.
/// If the server crashes or the worker hangs, the TCP close triggers
/// the client's `onclose` handler immediately — no polling needed.
pub async fn handle_status_websocket(
    req: Request<Body>,
    #[cfg(not(feature = "mock"))] bridge: SharedWorkerBridge,
    #[cfg(feature = "mock")] _bridge: (),
) -> Result<Response<Body>, Infallible> {
    if !is_websocket_upgrade(&req) {
        return Ok(build_json_error_response(
            StatusCode::BAD_REQUEST,
            "WebSocket upgrade required",
        ));
    }

    let key = get_websocket_key(&req).unwrap_or_default();
    let accept_key = calculate_websocket_accept_key(&key);

    #[cfg(not(feature = "mock"))]
    {
        let bridge_ws = bridge.clone();

        tokio::spawn(async move {
            match hyper::upgrade::on(req).await {
                Ok(upgraded) => {
                    if let Err(e) = handle_status_ws(upgraded, bridge_ws).await {
                        sys_error!("[WS_STATUS ERROR] {}", e);
                    }
                }
                Err(e) => {
                    sys_error!("[WS_STATUS UPGRADE ERROR] {}", e);
                }
            }
        });
    }

    #[cfg(feature = "mock")]
    {
        let _ = req;
    }

    Ok(build_websocket_upgrade_response(&accept_key))
}
