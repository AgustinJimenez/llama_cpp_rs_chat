# Testing Guide

## Quick Start

1. Start the server: `cargo run --bin llama_chat_web --features cuda`
2. Run API tests: `npx playwright test tests/e2e/tool-api.test.ts`

## Test Suites

### API Endpoint Tests (`tests/e2e/tool-api.test.ts`)

Tests all tool endpoints directly — no model required.

```bash
npx playwright test tests/e2e/tool-api.test.ts --reporter=list
```

**Result:** 50/50 tests pass across Chromium, Firefox, WebKit, Mobile Chrome, Mobile Safari.

Tools tested:
- **read_file** — read plain text, JSON, handle missing files
- **write_file** — create files, verify bytes written
- **list_directory** — non-recursive and recursive listing
- **bash** — echo, dir/ls, exit codes, stderr
- **Error handling** — unknown tools, missing args, file-not-found

### Agentic E2E Tests (`tests/e2e/agentic-tools.test.ts`)

Tests model autonomously using tools — requires a loaded model.

```bash
npx playwright test tests/e2e/agentic-tools.test.ts
```

Tests: read_file, list_directory, bash, multi-step workflows.

## Manual Browser Testing

1. Open http://localhost:4000
2. Load a model (Devstral, Qwen3-Coder, etc.)
3. Test prompts:
   - `"Read the file at E:\repo\llama_cpp_rs_chat\package.json"`
   - `"List all files in E:\repo\llama_cpp_rs_chat\src"`
   - `"Create a file called hello.txt with 'Hello World'"`

## Architecture

```
User Request → Model sees tools in system prompt
  → Model generates tool call (SYSTEM.EXEC / <tool_call> / <function=...>)
  → Backend parses + executes tool (native_tools.rs / command.rs)
  → Result injected back into conversation
  → Model generates final response
```

## Available Native Tools

| Tool | Description |
|------|-------------|
| `read_file` | Read any file from filesystem |
| `write_file` | Create or overwrite files |
| `list_directory` | List directory contents (recursive option) |
| `execute_python` | Run Python scripts via temp file |
| `execute_command` | Execute shell commands |

## Desktop Tool Notes

- MCP desktop tool execution is serialized. Only one desktop action runs at a time to avoid overlapping mouse/keyboard input when a previous action is slow or stuck.
- Desktop tool exposure is platform-filtered. Unsupported desktop automation tools are not advertised on platforms where they cannot work.
- On Windows, `type_text` prefers Unicode input for non-ASCII text and non-US keyboard layouts. This improves reliability for IME/non-QWERTY setups.
- `scroll_screen` now applies `dpi_aware` and `snap_to_screen` when `x` and `y` are provided, matching the click/move behavior.
- Known limitation: desktop tool timeouts do not fully cancel already-running blocking work. A timed-out OCR/UI automation call may continue running in the background, but serialization prevents it from overlapping with later desktop actions.
