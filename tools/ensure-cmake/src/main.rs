//! CLI wrapper: ensures cmake is available, then runs the given command.
//!
//! Usage:
//!   cargo run --manifest-path tools/ensure-cmake/Cargo.toml -- cargo build --features cuda

use std::env;
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();

    let cmake = match ensure_cmake::ensure_cmake(None) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("ERROR: {e}");
            return ExitCode::FAILURE;
        }
    };

    if args.is_empty() {
        if let Some(bin_dir) = &cmake.bin_dir {
            println!("{}", bin_dir.display());
        } else {
            eprintln!("cmake already on PATH");
        }
        return ExitCode::SUCCESS;
    }

    let (cmd, cmd_args) = args.split_first().unwrap();
    let mut command = Command::new(cmd);
    command.args(cmd_args);
    cmake.apply_to_command(&mut command);

    match command.status() {
        Ok(status) => ExitCode::from(status.code().unwrap_or(1) as u8),
        Err(e) => {
            eprintln!("ERROR: Failed to run '{cmd}': {e}");
            ExitCode::FAILURE
        }
    }
}
