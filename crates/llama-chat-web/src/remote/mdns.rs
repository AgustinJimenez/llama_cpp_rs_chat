//! mDNS/DNS-SD service announcement so phones on the same LAN can discover the server.

use mdns_sd::{ServiceDaemon, ServiceInfo};

const SERVICE_TYPE: &str = "_llama-chat._tcp.local.";

/// Announce the server on the local network via mDNS.
/// Runs in a background thread; call once at startup.
/// Returns the daemon handle — keep it alive for the process lifetime.
pub fn start(port: u16, local_ip: &str, hostname: &str) -> Option<ServiceDaemon> {
    let daemon = ServiceDaemon::new().ok()?;

    let host_fqdn = format!("{hostname}.local.");
    let properties = [("port", port.to_string().as_str())];

    let info = ServiceInfo::new(
        SERVICE_TYPE,
        "LlamaChat",
        &host_fqdn,
        local_ip,
        port,
        Some(&properties),
    )
    .ok()?;

    daemon.register(info).ok()?;

    log::info!("[mDNS] Announced {SERVICE_TYPE} on {local_ip}:{port}");
    Some(daemon)
}
