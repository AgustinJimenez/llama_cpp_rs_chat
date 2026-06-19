//! `rtk` (Rust Token Killer) output-compression prefix.
//!
//! Shell commands are wrapped with `rtk` to compress their output (fewer tokens for the LLM).
//! `rtk` is an open-source single-binary CLI (<https://github.com/rtk-ai/rtk>) vendored as a
//! git submodule at `deps/rtk` and built with the app — so it's NOT a system dependency.
//! We resolve the binary in this order:
//!   1. the vendored build at `deps/rtk/target/{release,debug}/rtk` (found relative to the exe),
//!   2. a global `rtk` on PATH (brew/cargo install),
//!   3. otherwise run the raw command uncompressed (so execution always works).

use std::path::PathBuf;
use std::sync::OnceLock;

/// Prefix `cmd` with the resolved `rtk` binary when available; otherwise return `cmd`
/// unchanged (the command runs without output compression).
pub fn rtk_prefix(cmd: &str) -> String {
    match rtk_path() {
        Some(rtk) => format!("{} {cmd}", shell_quote(rtk)),
        None => cmd.to_string(),
    }
}

/// Whether an `rtk` binary was resolved (vendored build or on PATH).
pub fn rtk_available() -> bool {
    rtk_path().is_some()
}

/// Absolute path (or bare name if on PATH) of the `rtk` binary — resolved once, cached.
fn rtk_path() -> Option<&'static str> {
    static PATH: OnceLock<Option<String>> = OnceLock::new();
    PATH.get_or_init(resolve_rtk).as_deref()
}

fn resolve_rtk() -> Option<String> {
    // 1. Vendored build next to the app: walk up from the executable looking for
    //    deps/rtk/target/{release,debug}/rtk.
    if let Ok(exe) = std::env::current_exe() {
        let mut dir: Option<PathBuf> = exe.parent().map(Path::to_path_buf);
        for _ in 0..8 {
            let Some(d) = dir else { break };
            for profile in ["release", "debug"] {
                let cand = d
                    .join("deps")
                    .join("rtk")
                    .join("target")
                    .join(profile)
                    .join(rtk_bin_name());
                if cand.is_file() {
                    return Some(cand.to_string_lossy().into_owned());
                }
            }
            dir = d.parent().map(Path::to_path_buf);
        }
    }
    // 2. Global install on PATH.
    if binary_on_path("rtk") {
        return Some("rtk".to_string());
    }
    None
}

use std::path::Path;

fn rtk_bin_name() -> &'static str {
    #[cfg(windows)]
    {
        "rtk.exe"
    }
    #[cfg(not(windows))]
    {
        "rtk"
    }
}

fn binary_on_path(bin: &str) -> bool {
    let Ok(path) = std::env::var("PATH") else {
        return false;
    };
    for dir in std::env::split_paths(&path) {
        if dir.join(bin).is_file() {
            return true;
        }
        #[cfg(windows)]
        {
            for ext in ["exe", "cmd", "bat"] {
                if dir.join(format!("{bin}.{ext}")).is_file() {
                    return true;
                }
            }
        }
    }
    false
}

/// Single-quote a path for `sh -c` if it contains characters that need it.
fn shell_quote(p: &str) -> String {
    if p.bytes().all(|b| b.is_ascii_alphanumeric() || b"/._-".contains(&b)) {
        p.to_string()
    } else {
        format!("'{}'", p.replace('\'', "'\\''"))
    }
}
