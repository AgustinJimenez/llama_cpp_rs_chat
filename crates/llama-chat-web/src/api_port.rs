//! The API port, and a preflight check so two instances can never quietly fight over it.
//!
//! # Why this exists
//!
//! The desktop app binds `127.0.0.1:18080`; the standalone web server binds
//! `0.0.0.0:18080`. Those are different addresses, so **both `bind()` calls succeed** and
//! neither process reports a problem. Windows then routes every `localhost` connection to
//! the more specific socket, so the desktop app silently answers all traffic and the web
//! server sits there looking healthy while receiving nothing.
//!
//! That cost most of a debugging session (AGENT_TASKS/015): from the moment the desktop app
//! was launched, every request in two separate investigations was served by a binary hours
//! old, producing "fixes that don't work", a retraction blaming an innocent build system,
//! and an apparently-changing agent list — because the other process had its own database.
//!
//! Nothing about that was visible from the client side. Hence: fail loudly, and allow a
//! second port so a dev server can run alongside an installed app.

use std::io::Read;
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::time::Duration;

/// Default API port when nothing overrides it.
pub const DEFAULT_API_PORT: u16 = 18080;

/// Environment variable that overrides the API port.
pub const PORT_ENV_VAR: &str = "LLAMA_CHAT_PORT";

/// The port the HTTP API should use.
///
/// Reads `LLAMA_CHAT_PORT`, falling back to [`DEFAULT_API_PORT`]. An unparseable or zero
/// value falls back rather than failing: a typo in an env var should not stop the app.
pub fn api_port() -> u16 {
    match std::env::var(PORT_ENV_VAR) {
        Ok(raw) => match raw.trim().parse::<u16>() {
            Ok(p) if p > 0 => p,
            _ => {
                eprintln!(
                    "[PORT] Ignoring invalid {PORT_ENV_VAR}={raw:?}; using {DEFAULT_API_PORT}"
                );
                DEFAULT_API_PORT
            }
        },
        Err(_) => DEFAULT_API_PORT,
    }
}

/// Who is already answering on `port`, if anyone.
///
/// Connects to `127.0.0.1` specifically — the address that actually matters, and the one a
/// wildcard bind will lose to. A successful connect means something is there, whatever this
/// process would manage to bind.
pub fn existing_listener(port: u16) -> Option<String> {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_millis(400)).ok()?;

    // Best-effort identification. /api/info carries `running_binary.pid` and `build_id`,
    // which is exactly what someone debugging this needs to see.
    let _ = stream.set_read_timeout(Some(Duration::from_millis(600)));
    let _ = std::io::Write::write_all(
        &mut stream,
        b"GET /api/info HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    let mut body = String::new();
    let _ = stream.take(8192).read_to_string(&mut body);

    let detail = body
        .split_once("\r\n\r\n")
        .map(|(_, b)| b.trim().to_string())
        .filter(|b| !b.is_empty())
        .unwrap_or_else(|| "(no /api/info response — not a llama-chat server?)".to_string());
    Some(detail)
}

/// Refuse to continue when something already owns the port.
///
/// # Errors
/// Returns a message naming the occupant when `127.0.0.1:port` already answers.
pub fn ensure_port_free(port: u16) -> Result<(), String> {
    match existing_listener(port) {
        None => Ok(()),
        Some(detail) => Err(format!(
            "Port {port} is already in use on 127.0.0.1 by another llama-chat instance.\n\
             \n\
             Binding 0.0.0.0:{port} would appear to succeed, but every localhost request \
             would still go to that process — and you would be testing ITS binary, not this \
             one. See AGENT_TASKS/015.\n\
             \n\
             Existing occupant reports:\n  {detail}\n\
             \n\
             Close the other instance (often the desktop app, `llama_chat_app.exe`), or run \
             this one on another port with {PORT_ENV_VAR}=<port>."
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unused_high_port_is_free() {
        // 0 is never a real listener; pick a port nothing should hold.
        assert!(ensure_port_free(49_871).is_ok());
    }

    #[test]
    fn occupied_port_is_detected() {
        let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        // Accept in the background so connect() completes.
        std::thread::spawn(move || {
            let _ = listener.accept();
        });
        let err = ensure_port_free(port).unwrap_err();
        assert!(err.contains(&port.to_string()), "message should name the port: {err}");
    }

    #[test]
    fn default_port_used_when_env_absent() {
        // Not asserting on a set value — the env is process-wide and tests run in parallel.
        assert_eq!(DEFAULT_API_PORT, 18080);
    }
}
