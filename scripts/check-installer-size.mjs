#!/usr/bin/env node
/**
 * Fail the build if the produced installer is implausibly large.
 *
 * A correct `dynamic-backends` installer is ~42 MB. A static-CUDA build is ~990 MB, because
 * the CUDA fatbins get linked into all seven bundled binaries instead of living in one
 * runtime-loaded `ggml-cuda.dll`. The two look identical from the command line — same
 * command shape, same exit code, same output path — so the only reliable signal is size.
 *
 * This exists because prose in AGENTS.md can be skimmed past, and shipping the 990 MB one is
 * the kind of mistake nobody notices until a user downloads it.
 */

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const NSIS_DIR = path.join(ROOT, 'target', 'release', 'bundle', 'nsis');

/** Above this, assume a static CUDA build slipped through. Correct build ≈ 42 MB. */
const LIMIT_MB = 150;

const mb = (b) => (b / 1024 / 1024).toFixed(1);

function main() {
  if (!fs.existsSync(NSIS_DIR)) {
    console.log('[check-installer-size] No NSIS output directory — nothing to check.');
    return;
  }

  const installers = fs
    .readdirSync(NSIS_DIR)
    .filter((n) => n.toLowerCase().endsWith('.exe'))
    .map((n) => {
      const p = path.join(NSIS_DIR, n);
      return { name: n, size: fs.statSync(p).size };
    });

  if (installers.length === 0) {
    console.log('[check-installer-size] No installer produced — nothing to check.');
    return;
  }

  let failed = false;
  for (const { name, size } of installers) {
    const sizeMb = size / 1024 / 1024;
    if (sizeMb > LIMIT_MB) {
      failed = true;
      console.error(
        `\n[check-installer-size] FAIL: "${name}" is ${mb(size)} MB (limit ${LIMIT_MB} MB).\n` +
          '\n' +
          'That is the signature of a STATIC CUDA build: the kernels get linked into every\n' +
          'bundled binary instead of one runtime-loaded ggml-cuda.dll.\n' +
          '\n' +
          'Rebuild with:  npm run tauri:build:dynamic\n' +
          'Do NOT use tauri:build / tauri:build:cuda / tauri:build:cpu for an installer.\n' +
          'See AGENT_TASKS/017 for the measurements and reasoning.\n',
      );
    } else {
      console.log(`[check-installer-size] OK: "${name}" is ${mb(size)} MB.`);
    }
  }

  if (failed) process.exit(1);
}

main();
