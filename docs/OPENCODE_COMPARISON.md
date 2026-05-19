# OpenCode Comparison

Date: 2026-05-19

Compared repositories:

- `E:\repo\llama_cpp_rs_chat`
- `E:\repo\opencode` (`https://github.com/anomalyco/opencode`)

## Scope

This was a static comparison only. `opencode` was cloned into `E:\repo\opencode` and inspected side by side with this repository. No code changes were made as part of the comparison.

## Verification Update

After the initial comparison note was written, the current working tree was checked again against the improvement list. Several of the earlier gaps have already been addressed in the present branch state, likely by the agent work done earlier the same day.

Important context from `current_task.md`:

- Active task focus has already moved to splitting large Rust files to satisfy the 800-line pre-commit limit.
- The CUDA/tool-injection deadlock task is documented there as mitigated, with follow-up cleanup still in progress.

Because of that, the sections below distinguish between:

- improvements that are now effectively present in the working tree
- improvements that are only partially complete
- improvements that still remain as real gaps

## High-Level Conclusion

`llama_cpp_rs_chat` is stronger in local model execution and runtime depth:

- Rust crate decomposition around inference, tools, worker process, web server, desktop tools, and DB
- llama.cpp / GGUF integration
- GPU backend handling
- out-of-process worker lifecycle
- dynamic tool-tag parsing and tool injection
- desktop automation and OCR depth

`opencode` is stronger in productization and engineering discipline around:

- monorepo structure and package boundaries
- CI coverage
- SDK and API contract surface
- docs presentation
- extension/plugin packaging
- repository hygiene

The main opportunity is not to copy `opencode` wholesale. The opportunity is to preserve this repo's strong Rust/local-inference core and improve the surrounding developer workflow, packaging, API surface, and maintainability.

## What OpenCode Does Better

### 1. Cleaner package boundaries

`opencode` uses a Bun/Turbo monorepo with clearly separated packages under `packages/`, including:

- `app`
- `desktop`
- `web`
- `ui`
- `sdk`
- `plugin`
- `docs`
- `opencode`

Relevant files:

- `E:\repo\opencode\package.json`
- `E:\repo\opencode\turbo.json`
- `E:\repo\opencode\packages\`

By comparison, `llama_cpp_rs_chat` has strong Rust crate boundaries in `crates/`, but the root `src/` still mixes:

- frontend app code
- Tauri entrypoints
- web entrypoints
- MCP UI server code
- helper binaries and tests

This is workable, but less clean than the separation seen in `opencode`.

### 2. Much stronger CI coverage

`opencode` has broad GitHub Actions coverage, including:

- unit tests on Linux and Windows
- e2e coverage
- artifact uploads
- separate workflows for publishing, review, docs, triage, and typecheck

Relevant files:

- `E:\repo\opencode\.github\workflows\test.yml`
- `E:\repo\opencode\.github\workflows\`

This repo no longer exposes only release automation.

Current repo state now includes:

- `E:\repo\llama_cpp_rs_chat\.github\workflows\ci.yml`
- `E:\repo\llama_cpp_rs_chat\.github\workflows\release.yml`

`ci.yml` currently covers:

- TypeScript typecheck
- ESLint
- `cargo check`
- `cargo clippy`
- `cargo test`

So this gap is now **partially closed**. The remaining difference versus `opencode` is breadth:

- no Windows CI lane yet
- no e2e/browser test lane in CI yet
- no artifact/report publishing comparable to `opencode`

### 3. SDK and API surface are more formalized

`opencode` has a dedicated SDK package and generated API client flow:

- `E:\repo\opencode\packages\sdk\js\package.json`
- `E:\repo\opencode\packages\sdk\js\src\`
- `E:\repo\opencode\packages\docs\openapi.json`

Its app layers consume a typed client rather than depending on informal endpoint knowledge.

This repo now does expose a formal API contract in the working tree:

- `E:\repo\llama_cpp_rs_chat\docs\openapi.json`
- `E:\repo\llama_cpp_rs_chat\src\utils\apiClient.ts`

This gap is therefore **mostly closed**.

Remaining differences versus `opencode`:

- the API contract/client do not yet appear to be packaged as a standalone public SDK
- the integration story is still less productized than `opencode`'s dedicated SDK package layout

### 4. Better docs as a product surface

`opencode` has a dedicated docs package and site-oriented docs structure:

- `E:\repo\opencode\packages\docs\`
- `E:\repo\opencode\packages\web\src\content\docs\`

This repo still has a large set of internal engineering notes:

- `E:\repo\llama_cpp_rs_chat\docs\`

Examples include:

- `MODEL_CONFIGURATIONS.md`
- `PIPELINE_REFERENCE.md`
- `PROVIDERS.md`
- `TESTING.md`

However, this gap is no longer fully accurate. A site-style docs surface now exists in:

- `E:\repo\llama_cpp_rs_chat\docs\site\index.md`
- `E:\repo\llama_cpp_rs_chat\docs\site\installation.md`
- `E:\repo\llama_cpp_rs_chat\docs\site\quickstart.md`
- `E:\repo\llama_cpp_rs_chat\docs\site\models.md`
- `E:\repo\llama_cpp_rs_chat\docs\site\providers.md`
- `E:\repo\llama_cpp_rs_chat\docs\site\mcp.md`
- `E:\repo\llama_cpp_rs_chat\docs\site\desktop-tools.md`
- `E:\repo\llama_cpp_rs_chat\docs\site\api.md`
- `E:\repo\llama_cpp_rs_chat\docs\site\troubleshooting.md`

So this is now **partially to mostly closed**, depending on how polished or published the docs site is intended to be.

### 5. Stronger test density

Rough comparison from file-level inspection:

- `opencode`: about 390 test/spec-style files found repo-wide
- `llama_cpp_rs_chat`: about 19 test/spec-style files found repo-wide

This is not a perfect metric, but it is directionally meaningful. `opencode` still appears to have substantially more test coverage and more clearly institutionalized validation.

This remains a **real gap**, even though CI has improved.

### 6. Cleaner repository hygiene

The `opencode` root is comparatively tidy and product-like.

This repo root still contains many logs, local artifacts, backups, and experimental files alongside source:

- `*.log`
- `*.llama_bak`
- temp screenshots
- scratch text files
- local binaries
- extra experiments and side artifacts

That said, `.gitignore` has been expanded significantly and now excludes many of these categories:

- logs
- local backups
- generated schemas
- screenshots/temp images
- local executables
- scratch outputs
- downloaded artifacts

So repo hygiene is now **improving, but not finished**. The ignore rules are better, while the working tree and root layout are still noisy.

### 7. More explicit extension/platform packaging

`opencode` has dedicated package surfaces for:

- SDK
- plugin
- docs
- desktop
- app
- web

This makes it easier to evolve external integration points.

This repo has strong built-in tools and MCP support, but the extension model is still more implicit than productized.

## What This Repo Already Does Better

The comparison should not hide this repository's strengths.

`llama_cpp_rs_chat` appears stronger than `opencode` in these areas:

- local GGUF model execution path
- direct llama.cpp integration
- VRAM-aware GPU layer calculation
- worker process isolation for reclaiming VRAM
- detailed tool-tag compatibility across local models
- native tool injection into generation loops
- desktop automation and OCR implementation depth
- model-specific prompt/template handling
- conversation compaction integrated with real token accounting

These are meaningful advantages. The surrounding engineering system should be improved without weakening the runtime architecture that already differentiates this project.

## Concrete Improvements For This Repo

Status legend used below:

- **Done in working tree**: present now, though possibly not fully merged/cleaned up
- **Partial**: meaningful progress exists, but the gap is not fully closed
- **Open**: still a real missing piece

### 1. Add real CI for normal development

Status: **Partial**

Highest-value improvement.

Already present:

- `cargo test`
- `cargo clippy`
- `cargo check`
- `npm run lint` equivalent via direct ESLint
- `npm run typecheck`

Still missing or not yet verified:

- Windows CI coverage
- e2e/browser coverage in CI
- mock-mode or integration-specific CI lanes
- richer artifacts/reports

Goal:

- catch regressions before release tags
- make contributions safer
- enforce baseline quality automatically

### 2. Publish a typed API contract

Status: **Done in working tree**

Now present:

- OpenAPI spec for REST endpoints
- generated TS client for the frontend and external consumers

Observed files:

- `E:\repo\llama_cpp_rs_chat\docs\openapi.json`
- `E:\repo\llama_cpp_rs_chat\src\utils\apiClient.ts`

Remaining improvement would be packaging/public distribution polish rather than initial implementation.

This would reduce drift between:

- web frontend
- Tauri integration points
- external automation
- future SDK or CLI consumers

It would also make the app's headless/programmatic mode much more usable.

### 3. Clean the repository root aggressively

Status: **Partial**

Create a clearer boundary between source and local artifacts.

Already improved:

- `.gitignore` has been expanded materially

Still needed:

- move logs into an ignored `artifacts/` or `tmp/` directory
- keep ad hoc experiments out of the repo root
- avoid checking in generated clutter near source files

This is a high-leverage maintainability improvement with low implementation cost.

### 4. Separate frontend app structure more cleanly

Status: **Partial**

Rust crate organization is already good. The frontend and app shell structure should catch up.

Already observed:

- command code split under `src/commands/`
- MCP UI code split under `src/mcp_ui/`

Still worth doing:

- isolating the React frontend into a more self-contained app directory
- reducing the number of unrelated entrypoints and helper files in root `src/`
- keeping Tauri shell code, web-only entrypoints, and UI app logic more clearly partitioned

This would improve onboarding and reduce accidental coupling.

### 5. Expand automated test coverage around fragile paths

Status: **Open**

Priority areas:

- REST API route behavior
- WebSocket streaming lifecycle
- provider streaming parity between web mode and Tauri mode
- command execution behavior on Windows
- tool loop iteration limits
- worker restart / hard-unload behavior
- tool-tag parsing across model families

This repo already has a `mock` feature and some scaffolding. It should be used more aggressively.

### 6. Turn docs into an external-facing docs experience

Status: **Partial to Done in working tree**

The current docs folder is no longer only internal notes. A site-style docs structure now exists under `docs/site/`.

A better docs surface should cover:

- install paths by platform
- development quickstart
- model loading and configuration
- provider setup
- MCP setup
- desktop tools
- REST API
- troubleshooting

That can start as a simpler static docs site without changing the technical content much.

### 7. Create a more explicit extension story

Status: **Open**

Potential directions:

- provider plugin API
- tool pack/plugin contract
- generated SDK for app automation
- stable external integration points for MCP/UI/API consumers

This repo already has strong underlying capability. What is missing is a stable and documented packaging story for others to build on top of it.

## What Not To Copy Blindly

This repo should not be restructured just to resemble `opencode`.

Do not trade away:

- Rust-first runtime architecture
- local inference specialization
- worker isolation
- GGUF metadata-driven configuration
- GPU/backend-specific optimizations
- native desktop tooling depth

The right direction is:

- keep the current core strengths
- import better CI, packaging, docs, API discipline, and repo hygiene

## Revised Priority Order

1. Test expansion around mock-mode, streaming, providers, and tool loops
2. Finish repo cleanup beyond `.gitignore` improvements
3. Continue frontend/app boundary cleanup from the current modularization work
4. Add Windows and e2e lanes to CI
5. Decide whether the OpenAPI/client layer should become a packaged public SDK
6. Finish productizing the docs site if it is intended for external use
7. Define an explicit extension/plugin surface

## Summary

`opencode` is ahead on engineering system maturity.

`llama_cpp_rs_chat` is ahead on local-model runtime sophistication.

The best improvements for this repo are therefore now:

- better automated test coverage
- broader CI coverage
- cleaner repo hygiene in practice, not just ignore rules
- clearer package/app boundaries
- a more explicit extension story

Items that appear already implemented in the current working tree:

- baseline CI
- formal OpenAPI contract
- generated TS API client
- initial public-facing docs site structure
