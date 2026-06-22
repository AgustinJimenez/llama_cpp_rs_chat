//! UPnP IGD port mapping + STUN public IP discovery.

use std::net::{Ipv4Addr, SocketAddrV4};

use igd_next::aio::tokio::search_gateway;
use igd_next::{PortMappingProtocol, SearchOptions};

const MAPPING_DESCRIPTION: &str = "llama-chat";
const STUN_SERVER: &str = "stun.l.google.com:19302";

#[derive(Debug)]
pub struct RemoteAccess {
    pub external_ip: String,
    pub external_port: u16,
}

/// Try UPnP port mapping + STUN to expose the local server to the internet.
/// Returns the public URL on success or an error string.
pub async fn enable(local_ip: Ipv4Addr, local_port: u16) -> Result<RemoteAccess, String> {
    // 1. Find the UPnP gateway (home router)
    let gateway = search_gateway(SearchOptions::default())
        .await
        .map_err(|e| format!("UPnP gateway not found: {e}. Check that UPnP is enabled on your router."))?;

    let local_addr = SocketAddrV4::new(local_ip, local_port);

    // 2. Request port mapping (0 = no expiry)
    let ext_port = gateway
        .add_any_port(PortMappingProtocol::TCP, local_addr, 0, MAPPING_DESCRIPTION)
        .await
        .map_err(|e| format!("UPnP port mapping failed: {e}"))?;

    // 3. STUN to get the public IP
    let external_ip = discover_public_ip().await?;

    Ok(RemoteAccess {
        external_ip,
        external_port: ext_port,
    })
}

/// Remove a previously added UPnP port mapping.
pub async fn disable(external_port: u16) -> Result<(), String> {
    let gateway = search_gateway(SearchOptions::default())
        .await
        .map_err(|e| format!("UPnP gateway not found: {e}"))?;

    gateway
        .remove_port(PortMappingProtocol::TCP, external_port)
        .await
        .map_err(|e| format!("Failed to remove UPnP mapping: {e}"))
}

/// Use STUN to discover the machine's public IP address.
/// Runs blocking DNS + STUN query on a thread pool to avoid blocking tokio.
async fn discover_public_ip() -> Result<String, String> {
    tokio::task::spawn_blocking(|| {
        use std::net::ToSocketAddrs as _;

        let stun_addr = STUN_SERVER
            .to_socket_addrs()
            .map_err(|e| format!("STUN DNS lookup failed: {e}"))?
            .next()
            .ok_or("STUN server has no addresses")?;

        let socket = std::net::UdpSocket::bind("0.0.0.0:0")
            .map_err(|e| format!("Failed to bind UDP socket: {e}"))?;

        let client = stunclient::StunClient::new(stun_addr);
        let external = client
            .query_external_address(&socket)
            .map_err(|e| format!("STUN query failed: {e:?}"))?;

        Ok::<String, String>(external.ip().to_string())
    })
    .await
    .map_err(|e| format!("STUN task panicked: {e}"))?
}
