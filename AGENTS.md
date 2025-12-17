AGENTS GUIDE

This is the short canonical reference for agents (Claude Code, OpenAI Agents, etc.) working in this repo. Use this to remember key development steps across sessions.

Default development: run "npm run dev" from the repo root. This starts Vite on port 4000 with hot reload and the Rust backend on port 8000. Always access the app via http://localhost:4000 during development. Port 8000 serves only the backend API.

Frontend alternatives: "npm run build" for production build. If the backend is already running, start Vite only with "npx vite --host --port 4000".

Backend alternatives: "cargo run --bin llama_chat_web" if Vite is already running. Rust builds: "cargo build --lib" and "cargo build --bin llama_chat_web".

Desktop app: "cargo tauri dev" for hot reload, "cargo tauri build" for production desktop.

Testing: "npm test" (Playwright E2E; backend must be running on 8000). UI/headed/debug variants: "npm run test:ui", "npm run test:headed", "npm run test:debug", "npm run test:docker". Unit tests: "cargo test". Single browser: "npx playwright test --project=chromium".

Mock mode for tests: build or run with the "mock" feature. Example: "cargo build --features mock --bin llama_chat_web" or "TEST_MODE=true cargo run --bin llama_chat_web".

CMake on Windows: set CMAKE environment variable to your cmake.exe path if not in PATH (e.g., "C:\Program Files\CMake\bin\cmake.exe").

Tool calling: tool schema is exposed via /api/tools/available and execution via /api/tools/execute. Models see available tools injected into prompts. Safety limit MAX_TOOL_ITERATIONS = 5 on the frontend agent loop.

Common gotchas to remember: use port 4000 for the UI (not 8000), keep backend running for Playwright tests, and prefer existing modules rather than duplicating code when editing web routes or chat logic.
