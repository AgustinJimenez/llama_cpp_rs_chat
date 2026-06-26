//! Terminal WebSocket route — upgrades GET /ws/terminal to a PTY-backed session.

use std::convert::Infallible;

use hyper::{Body, Request, Response, StatusCode};

use crate::websocket::handle_terminal_ws;
use crate::websocket_utils::{
    build_json_error_response, build_websocket_upgrade_response, calculate_websocket_accept_key,
    get_websocket_key, is_websocket_upgrade,
};

pub async fn handle_terminal_websocket(
    req: Request<Body>,
) -> Result<Response<Body>, Infallible> {
    if !is_websocket_upgrade(&req) {
        return Ok(build_json_error_response(
            StatusCode::BAD_REQUEST,
            "WebSocket upgrade required",
        ));
    }

    let key = get_websocket_key(&req).unwrap_or_default();
    let accept_key = calculate_websocket_accept_key(&key);

    tokio::spawn(async move {
        match hyper::upgrade::on(req).await {
            Ok(upgraded) => {
                if let Err(e) = handle_terminal_ws(upgraded).await {
                    sys_error!("[WS_TERMINAL ERROR] {}", e);
                }
            }
            Err(e) => {
                sys_error!("[WS_TERMINAL UPGRADE ERROR] {}", e);
            }
        }
    });

    Ok(build_websocket_upgrade_response(&accept_key))
}
