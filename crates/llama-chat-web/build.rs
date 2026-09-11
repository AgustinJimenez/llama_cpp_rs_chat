//! Emits `LLAMA_CHAT_BUILD_ID`, a value that changes on every compile of this crate.
//!
//! Exists to answer one question that was unanswerable during AGENT_TASKS/012 and 013:
//! *is the process serving my requests actually running the code I just built?* Both
//! investigations lost significant time to fixes that appeared present in the on-disk
//! binary yet behaved like the old code, and nothing available at runtime could confirm or
//! deny it. `/api/info` now reports this, so an experiment can assert it before trusting
//! any result.
//!
//! `rerun-if-changed=.` keeps the id in step with source edits without rebuilding the
//! world on every unrelated `cargo` invocation.

use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // Short git revision when available — makes the id meaningful in a bug report, not just
    // unique. Absent in a tarball build, which is fine: the timestamp alone still answers
    // the staleness question.
    let rev = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "nogit".to_string());

    println!("cargo:rustc-env=LLAMA_CHAT_BUILD_ID={rev}-{stamp}");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=build.rs");
}
