//! Remote access API routes: status, UPnP enable/disable, token regenerate.

use std::convert::Infallible;

use hyper::{Body, Request, Response, StatusCode};

use llama_chat_db::SharedDatabase;

use crate::remote;
use crate::remote::upnp;
use crate::response_helpers::{json_error, json_response};

const SERVER_PORT: u16 = 18080;

/// GET /api/remote/status
pub async fn handle_get_status(db: SharedDatabase) -> Result<Response<Body>, Infallible> {
    let token = remote::get_or_create_token(&db);
    let lan_url = remote::lan_url(SERVER_PORT);

    let body = serde_json::json!({
        "token": token,
        "lan_url": lan_url,
        "lan_qr": lan_url.as_ref().map(|url| format!("{url}#token={token}")),
    });

    Ok(json_response(StatusCode::OK, &body))
}

/// POST /api/remote/upnp/enable
pub async fn handle_upnp_enable(db: SharedDatabase) -> Result<Response<Body>, Infallible> {
    let local_ip = match remote::get_local_ip() {
        Some(std::net::IpAddr::V4(ip)) => ip,
        Some(_) => {
            return Ok(json_error(StatusCode::BAD_REQUEST, "IPv6-only interface not supported for UPnP"));
        }
        None => {
            return Ok(json_error(StatusCode::INTERNAL_SERVER_ERROR, "Could not determine local IP address"));
        }
    };

    match upnp::enable(local_ip, SERVER_PORT).await {
        Ok(access) => {
            let token = remote::get_or_create_token(&db);
            let public_url = format!("http://{}:{}", access.external_ip, access.external_port);
            let body = serde_json::json!({
                "success": true,
                "public_url": public_url,
                "public_qr": format!("{public_url}#token={token}"),
                "external_ip": access.external_ip,
                "external_port": access.external_port,
            });
            Ok(json_response(StatusCode::OK, &body))
        }
        Err(e) => Ok(json_error(StatusCode::INTERNAL_SERVER_ERROR, &e)),
    }
}

/// POST /api/remote/upnp/disable  body: {"external_port": 18080}
pub async fn handle_upnp_disable(req: Request<Body>) -> Result<Response<Body>, Infallible> {
    let body_bytes = hyper::body::to_bytes(req.into_body()).await.unwrap_or_default();
    let port: u16 = serde_json::from_slice::<serde_json::Value>(&body_bytes)
        .ok()
        .and_then(|v| v["external_port"].as_u64())
        .and_then(|p| u16::try_from(p).ok())
        .unwrap_or(SERVER_PORT);

    match upnp::disable(port).await {
        Ok(()) => Ok(json_response(StatusCode::OK, &serde_json::json!({"success": true}))),
        Err(e) => Ok(json_error(StatusCode::INTERNAL_SERVER_ERROR, &e)),
    }
}

/// POST /api/remote/token/regenerate
pub async fn handle_regenerate_token(db: SharedDatabase) -> Result<Response<Body>, Infallible> {
    let token = uuid::Uuid::new_v4().to_string().replace('-', "");
    match db.set_remote_access_token(&token) {
        Ok(()) => Ok(json_response(StatusCode::OK, &serde_json::json!({"token": token}))),
        Err(e) => Ok(json_error(StatusCode::INTERNAL_SERVER_ERROR, &e)),
    }
}
