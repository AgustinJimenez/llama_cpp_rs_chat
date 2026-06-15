#!/usr/bin/env node
/**
 * ensure-rtk — vendor + build the `rtk` (Rust Token Killer) output-compression CLI so it
 * ships WITH the app instead of being a system dependency.
 *
 * Steps (idempotent — safe to run on every build):
 *   1. Make sure the `deps/rtk` git submodule is checked out.
 *   2. Ensure `deps/rtk/Cargo.toml` has a `[workspace]` table so cargo builds it standalone
 *      (the repo root is itself a workspace member `.`, so `exclude` can't carve out a subdir;
 *      a `[workspace]` table in the sub-manifest is how `deps/llama-cpp-rs` is isolated too).
 *   3. Build the release binary if it's missing.
 *
 * The app resolves the built binary at runtime via `deps/rtk/target/release/rtk`
 * (crates/llama-chat-command/src/rtk.rs), falling back to PATH, then to raw commands.
 */
const { execSync } = require('child_process');
const fs = require('fs');
const path = require('path');

const repoRoot = path.resolve(__dirname, '..');
const rtkDir = path.join(repoRoot, 'deps', 'rtk');
const rtkCargo = path.join(rtkDir, 'Cargo.toml');
const binName = process.platform === 'win32' ? 'rtk.exe' : 'rtk';
const rtkBin = path.join(rtkDir, 'target', 'release', binName);

function run(cmd, cwd) {
  execSync(cmd, { cwd: cwd || repoRoot, stdio: 'inherit' });
}

try {
  // 1. Submodule checkout
  if (!fs.existsSync(rtkCargo)) {
    console.log('[ensure-rtk] checking out deps/rtk submodule...');
    run('git submodule update --init deps/rtk');
  }
  if (!fs.existsSync(rtkCargo)) {
    console.warn('[ensure-rtk] deps/rtk not available; skipping (commands will run uncompressed).');
    process.exit(0);
  }

  // 2. Standalone-workspace patch
  const cargo = fs.readFileSync(rtkCargo, 'utf8');
  if (!/^\s*\[workspace\]/m.test(cargo)) {
    console.log('[ensure-rtk] adding [workspace] to deps/rtk/Cargo.toml (build isolation)...');
    fs.appendFileSync(rtkCargo, '\n[workspace]\n');
  }

  // 3. Build if missing
  if (fs.existsSync(rtkBin)) {
    console.log('[ensure-rtk] rtk already built.');
  } else {
    console.log('[ensure-rtk] building rtk (release)...');
    run('cargo build --release --manifest-path deps/rtk/Cargo.toml');
  }
  console.log(`[ensure-rtk] ready: ${rtkBin}`);
} catch (e) {
  // Non-fatal: the app degrades to running commands without rtk compression.
  console.warn(`[ensure-rtk] failed (non-fatal): ${e.message}`);
  process.exit(0);
}
