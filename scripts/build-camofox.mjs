#!/usr/bin/env node
/**
 * Build camofox-browser into a standalone binary using Node.js SEA + esbuild.
 *
 * Usage:
 *   node scripts/build-camofox.mjs
 *
 * Output:
 *   binaries/camofox-server-x86_64-pc-windows-msvc.exe  (Windows)
 *   binaries/camofox-server-x86_64-unknown-linux-gnu     (Linux)
 *   binaries/camofox-server-aarch64-apple-darwin          (macOS ARM)
 *
 * The binary is a standalone HTTP server (port 9377) that wraps Camoufox
 * (anti-detection Firefox). On first run it downloads the Camoufox browser
 * binary (~300MB). No Node.js needed on the target machine.
 */

import { execSync, spawnSync } from "child_process";
import { existsSync, mkdirSync, copyFileSync, writeFileSync, readFileSync, rmSync } from "fs";
import { resolve, join } from "path";

const ROOT = resolve(import.meta.dirname, "..");
const BUILD_DIR = join(ROOT, ".camofox-build");
const BINARIES_DIR = join(ROOT, "binaries");
const REPO_URL = "https://github.com/jo-inc/camofox-browser.git";

// Detect platform for Tauri sidecar naming
function getTauriTarget() {
  const platform = process.platform;
  const arch = process.arch;
  if (platform === "win32") return "x86_64-pc-windows-msvc";
  if (platform === "darwin" && arch === "arm64") return "aarch64-apple-darwin";
  if (platform === "darwin") return "x86_64-apple-darwin";
  if (platform === "linux" && arch === "arm64") return "aarch64-unknown-linux-gnu";
  return "x86_64-unknown-linux-gnu";
}

function run(cmd, opts = {}) {
  console.log(`  > ${cmd}`);
  execSync(cmd, { stdio: "inherit", cwd: opts.cwd || ROOT, ...opts });
}

async function main() {
  const target = getTauriTarget();
  const isWindows = process.platform === "win32";
  const ext = isWindows ? ".exe" : "";
  const outName = `camofox-server-${target}${ext}`;

  console.log(`\n=== Building Camofox Sidecar for ${target} ===\n`);

  // Step 1: Clone or update camofox-browser
  if (!existsSync(BUILD_DIR)) {
    console.log("1. Cloning camofox-browser...");
    run(`git clone --depth 1 ${REPO_URL} "${BUILD_DIR}"`);
  } else {
    console.log("1. Updating camofox-browser...");
    run("git pull --ff-only", { cwd: BUILD_DIR });
  }

  // Step 2: Install dependencies
  console.log("\n2. Installing dependencies...");
  run("npm install --production", { cwd: BUILD_DIR });

  // Step 3: Bundle with esbuild into a single CJS file
  console.log("\n3. Bundling with esbuild...");

  // Install esbuild locally if not present
  if (!existsSync(join(BUILD_DIR, "node_modules", ".package-lock.json")) ||
      spawnSync("npx", ["esbuild", "--version"], { cwd: BUILD_DIR, stdio: "pipe" }).status !== 0) {
    run("npm install --save-dev esbuild", { cwd: BUILD_DIR });
  }

  const bundlePath = join(BUILD_DIR, "dist", "camofox-bundle.mjs");
  mkdirSync(join(BUILD_DIR, "dist"), { recursive: true });

  // Must use ESM format (server.js uses top-level await) and mark problematic deps as external
  run(
    [
      `npx esbuild server.js --bundle --platform=node --format=esm`,
      `--outfile=dist/camofox-bundle.mjs`,
      `--external:fsevents`,
      `--external:bun:sqlite`,
      `--external:chromium-bidi`,
      `--external:impit-win32-x64-msvc`,
      `--external:impit-linux-x64-gnu`,
      `--external:impit-darwin-arm64`,
      `--external:impit-darwin-x64`,
      `--loader:.node=empty`,
      `--banner:js="import{createRequire}from'module';const require=createRequire(import.meta.url);"`,
    ].join(" "),
    { cwd: BUILD_DIR }
  );

  if (!existsSync(bundlePath)) {
    console.error("ERROR: esbuild bundle not created");
    process.exit(1);
  }

  const bundleSize = readFileSync(bundlePath).length;
  console.log(`   Bundle size: ${(bundleSize / 1024 / 1024).toFixed(1)} MB`);

  // Step 4: Create SEA config
  console.log("\n4. Creating SEA config...");
  const seaConfig = {
    main: "dist/camofox-bundle.mjs",
    output: "dist/sea-prep.blob",
    disableExperimentalSEAWarning: true,
    useSnapshot: false,
    useCodeCache: true,
  };
  writeFileSync(join(BUILD_DIR, "sea-config.json"), JSON.stringify(seaConfig, null, 2));

  // Step 5: Generate SEA blob
  console.log("\n5. Generating SEA blob...");
  run("node --experimental-sea-config sea-config.json", { cwd: BUILD_DIR });

  const blobPath = join(BUILD_DIR, "dist", "sea-prep.blob");
  if (!existsSync(blobPath)) {
    console.error("ERROR: SEA blob not created");
    process.exit(1);
  }

  // Step 6: Copy node binary and inject SEA blob
  console.log("\n6. Creating standalone binary...");
  mkdirSync(BINARIES_DIR, { recursive: true });

  const nodeBin = process.execPath;
  const outputBin = join(BINARIES_DIR, outName);

  // Copy node binary
  copyFileSync(nodeBin, outputBin);

  if (isWindows) {
    // Remove signature (required for SEA injection on Windows)
    try {
      run(`signtool remove /s "${outputBin}"`, { stdio: "pipe" });
    } catch {
      // signtool may not be available; try postject anyway
      console.log("   (signtool not found, trying injection anyway)");
    }
  } else {
    // Make executable
    run(`chmod +x "${outputBin}"`);
  }

  // Inject SEA blob using postject
  run("npm install --save-dev postject", { cwd: BUILD_DIR });

  const sentinelFuse = "NODE_SEA_FUSE_fce680ab2cc467b6e072b8b5df1996b2";
  const postjectArgs = [
    `"${outputBin}"`,
    "NODE_SEA_BLOB",
    `"${blobPath}"`,
    "--sentinel-fuse",
    sentinelFuse,
  ];

  if (isWindows) {
    postjectArgs.push("--overwrite");
  }
  if (process.platform === "darwin") {
    postjectArgs.push("--sentinel-fuse", sentinelFuse);
    postjectArgs.push("--macho-segment-name", "NODE_SEA");
  }

  run(`npx postject ${postjectArgs.join(" ")}`, { cwd: BUILD_DIR });

  // Verify output
  if (!existsSync(outputBin)) {
    console.error("ERROR: Final binary not created");
    process.exit(1);
  }

  const finalSize = readFileSync(outputBin).length;
  console.log(`\n=== Done! ===`);
  console.log(`Binary: ${outputBin}`);
  console.log(`Size: ${(finalSize / 1024 / 1024).toFixed(1)} MB`);
  console.log(`\nTo test: ${outputBin}`);
  console.log("First run will download Camoufox browser (~300MB).\n");
}

main().catch((err) => {
  console.error("Build failed:", err);
  process.exit(1);
});
