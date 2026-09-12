# 016 — The installer bundles every binary in `target/release`, not just the app

Status: DIAGNOSED 2026-09-12 — root cause proven, fix NOT applied (needs a package split)
Found: 2026-09-12, after the user asked why the installer is ~1 GB

## Symptom

`LLaMA Chat_0.1.0_x64-setup.exe` is **989.7 MB**. Contents, read with 7-Zip:

| Binary | MB | Needed by the app? |
|---|---|---|
| `llama_chat_app.exe` | 208.8 | **yes** |
| `llama_chat_web.exe` | 201.9 | no — standalone web server |
| `mcp_desktop_tools.exe` | 183.6 | no — MCP server for Claude Code |
| `mcp_desktop_smoke.exe` | 181.1 | no — smoke test |
| `test_sample_deadlock.exe` | 158.2 | no |
| `test_model.exe` | 158.1 | no |
| `test_vision.exe` | 158.1 | no |

**1,249.8 MB uncompressed → 989.7 MB installer. ~780 MB (≈79%) is dead weight.**

Each is large for the same reason: they all link CUDA-enabled llama.cpp, so every one carries
a full fatbin payload. This is the bloat `AGENTS.md` already warned about ("especially if
test binaries are compiled into target/release") — but the warning had no mechanism behind
it, so it recurred.

## Root cause — the bundler does TWO things, and both must be defeated

Proven by experiment, not by reading Tauri's source:

1. **Declared `[[bin]]` targets are required.** Removing `llama_chat_web.exe` from
   `target/release` makes bundling fail outright:
   `Error failed to bundle project: when getting size of …\llama_chat_web.exe`.
   So every `[[bin]]` in the root `Cargo.toml` is enumerated and demanded.
2. **Undeclared stray `.exe` files are swept in anyway.** `mcp_desktop_smoke.exe` and
   `test_chat_template.exe` appear in **no** `Cargo.toml` in the workspace — they are
   leftovers from bin targets that were deleted — yet both ship in the installer.

## What does NOT work (all tried and measured)

| Attempt | Result |
|---|---|
| `cargo tauri build … -- --bin llama_chat_app` | Ignored. All bins still built and bundled; installer unchanged at 989.7 MB. |
| `beforeBundleCommand` hook that moves the extras aside | Runs **after** the file list is computed. Bundling then fails on the missing file. |
| Prune first, then `cargo tauri build … -- --bin …` | Cargo re-links every bin from its artifact cache (restored files reappear with their cached mtimes), so they are back before bundling. |
| `cargo build --bin llama_chat_app` → prune → `cargo tauri bundle` | `cargo build --bin` works correctly, but `tauri bundle` still demands `llama_chat_web.exe` and fails. |

The last row is the important one: the separate `bundle` subcommand does not help, because
requirement (1) is independent of how the build ran.

## The actual fix — split the package

The root package is simultaneously the Tauri app **and** the home of the web server, the MCP
server, and the test binaries. Requirement (1) means the only way to keep those out of the
installer is for them not to be `[[bin]]` targets of the Tauri package.

Move them into a sibling crate — e.g. `crates/llama-chat-bins/` — leaving the root package
with `llama_chat_app` alone. Expected result at the measured ~79 % compression ratio:
**~165–175 MB instead of 989.7 MB.**

Care points:
- `npm run build:cuda` builds `--bin llama_chat_web`; it must point at the new package.
- Whatever builds `mcp_desktop_tools` for the Claude Code MCP server must follow.
- The root `build.rs` and the `cuda`/`vision`/`dynamic-backends` feature wiring are shared and
  must stay consistent across both packages.
- Delete the orphaned `mcp_desktop_smoke.exe` / `test_chat_template.exe` from
  `target/release`; they are rebuilt by nothing and only add bulk.

`scripts/prune-release-bins.mjs` is kept: it is still the right cleanup for stray
undeclared binaries (requirement 2), and it prints a loud warning for any file it cannot
move. It is simply not sufficient on its own.

## Separately: the app binary itself is 208 MB because CUDA is compiled in

Distinct from this task, and worth its own decision. `Cargo.toml:215` already defines
`dynamic-backends = ["llama-cpp-2/dynamic-backends", …]` — *"Runtime GPU backend loading
(CUDA/Vulkan as DLLs)"*. Building with it would load `ggml-cuda.dll` from the host at runtime
rather than baking the kernels in, which is what "rely on what the machine already has"
actually requires. `AGENTS.md` currently says CUDA DLLs are not bundled, which is true, but
the fatbins are inside the executable regardless.

Not attempted: it changes how the GPU backend loads and needs its own verification pass.

## Verification

- Build the installer; assert with 7-Zip that it contains exactly one `.exe`
  (`llama_chat_app.exe`) plus the four MSVC runtime DLLs.
- Assert the installed app still loads a model on GPU.
- Assert `npm run build:cuda` still produces a working `llama_chat_web`.
