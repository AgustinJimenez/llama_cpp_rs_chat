//! Remote access: LAN mDNS discovery, UPnP port mapping, STUN public IP, auth token management.

pub mod mdns;
pub mod upnp;

use std::net::IpAddr;

use llama_chat_db::SharedDatabase;

/// Get the best local non-loopback IPv4 address using the routing table trick.
/// Sends no data — just uses the kernel routing table.
pub fn get_local_ip() -> Option<IpAddr> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    socket.local_addr().ok().map(|a| a.ip())
}

/// Get or generate the remote access token, persisted in SQLite.
pub fn get_or_create_token(db: &SharedDatabase) -> String {
    if let Some(token) = db.get_remote_access_token() {
        if !token.is_empty() {
            return token;
        }
    }
    let token = uuid::Uuid::new_v4().to_string().replace('-', "");
    let _ = db.set_remote_access_token(&token);
    token
}

/// Check if a remote request carries the correct Bearer token.
pub fn check_bearer_token(auth_header: Option<&str>, expected: &str) -> bool {
    let Some(header) = auth_header else { return false };
    let Some(token) = header.strip_prefix("Bearer ") else { return false };
    // Constant-time comparison to resist timing attacks
    token.len() == expected.len()
        && token
            .bytes()
            .zip(expected.bytes())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
}

/// Build the LAN connection URL for display / QR code.
pub fn lan_url(port: u16) -> Option<String> {
    get_local_ip().map(|ip| format!("http://{ip}:{port}"))
}
