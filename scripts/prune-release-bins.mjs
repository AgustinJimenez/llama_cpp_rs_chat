#!/usr/bin/env node
/**
 * Move every `target/release/*.exe` except the app out of the way before Tauri bundles.
 *
 * Why: the NSIS bundler sweeps up *every* executable sitting in the target directory, not
 * just the one the app needs. A measured build shipped seven binaries — llama_chat_app,
 * llama_chat_web, mcp_desktop_tools, mcp_desktop_smoke and three test binaries — totalling
 * 1,249.8 MB uncompressed and producing a 989.7 MB installer, of which roughly 780 MB was
 * dead weight. Every one is large for the same reason: they all link CUDA-enabled
 * llama.cpp, so each carries a full fatbin payload.
 *
 * This is easy to reintroduce, because any earlier `cargo build` or `cargo test` leaves
 * those binaries behind and the bundler silently includes whatever it finds. Hence a hook
 * (`beforeBundleCommand`) rather than a note in a README.
 *
 * Binaries are MOVED, not deleted, so a later `cargo run --bin llama_chat_web` still works
 * after restoring them, and nothing has to be rebuilt to get them back.
 */

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const RELEASE_DIR = path.join(ROOT, 'target', 'release');
const PARKING = path.join(RELEASE_DIR, '_not-bundled');

/** The only executable the desktop bundle should contain. */
const KEEP = new Set(['llama_chat_app.exe']);

const mb = (bytes) => (bytes / 1024 / 1024).toFixed(1);

function main() {
  if (!fs.existsSync(RELEASE_DIR)) {
    console.log('[prune-release-bins] No target/release yet — nothing to do.');
    return;
  }

  const candidates = fs
    .readdirSync(RELEASE_DIR, { withFileTypes: true })
    .filter((e) => e.isFile() && e.name.toLowerCase().endsWith('.exe'))
    .map((e) => e.name)
    .filter((name) => !KEEP.has(name));

  if (candidates.length === 0) {
    console.log('[prune-release-bins] Only the app binary present — nothing to move.');
    return;
  }

  fs.mkdirSync(PARKING, { recursive: true });

  let moved = 0;
  let movedBytes = 0;
  const stuck = [];

  for (const name of candidates) {
    const from = path.join(RELEASE_DIR, name);
    const to = path.join(PARKING, name);
    let size = 0;
    try {
      size = fs.statSync(from).size;
    } catch {
      continue;
    }
    try {
      // Windows allows renaming a running executable, so a live MCP server or dev
      // server does not block this. An existing parked copy is replaced.
      fs.rmSync(to, { force: true });
      fs.renameSync(from, to);
      moved += 1;
      movedBytes += size;
      console.log(`[prune-release-bins] moved ${name} (${mb(size)} MB)`);
    } catch (err) {
      stuck.push({ name, size, err: err.message });
    }
  }

  console.log(
    `[prune-release-bins] ${moved} binaries moved to target/release/_not-bundled ` +
      `(~${mb(movedBytes)} MB kept out of the installer)`,
  );

  if (stuck.length > 0) {
    // Loud on purpose: a stuck binary silently ends up in the installer, which is the
    // exact failure this script exists to prevent.
    console.warn(
      `\n[prune-release-bins] WARNING: ${stuck.length} binary/binaries could NOT be moved ` +
        'and WILL be bundled, inflating the installer:',
    );
    for (const s of stuck) {
      console.warn(`  - ${s.name} (${mb(s.size)} MB): ${s.err}`);
    }
    console.warn('  Close whatever is holding them and rebuild.\n');
  }
}

main();
