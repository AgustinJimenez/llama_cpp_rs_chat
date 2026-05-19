# MCP (Model Context Protocol)

Connect external MCP servers to extend the AI's tool set.

---

## What is MCP?

MCP is a standard protocol for exposing tools to AI models. Any MCP-compatible server can be connected and its tools become available in the chat.

---

## Adding an MCP server

1. Open **Settings → MCP Servers**
2. Click **Add Server**
3. Enter:
   - **Name**: display name
   - **Command**: the executable to run (e.g. `npx`, `python`, `node`)
   - **Args**: command arguments (e.g. `["@modelcontextprotocol/server-filesystem", "/path/to/dir"]`)
   - **Transport**: `stdio` (default) or `sse`
4. Click **Save** — the server starts automatically

---

## Example: filesystem server

Gives the model access to a specific directory tree.

```json
{
  "name": "filesystem",
  "command": "npx",
  "args": ["-y", "@modelcontextprotocol/server-filesystem", "/home/user/projects"],
  "transport": "stdio"
}
```

---

## Example: custom HTTP server (SSE transport)

```json
{
  "name": "my-tools",
  "url": "http://localhost:9000/mcp",
  "transport": "sse"
}
```

---

## Embedded UI MCP server

When running as a Tauri desktop app, a built-in MCP server is exposed on **http://localhost:18091/mcp**.

This server lets external tools (like Claude Code) interact directly with the app's UI:

| Tool | Description |
|------|-------------|
| `app_get_state` | Get model load status, generation state |
| `app_load_model` | Load a model by path |
| `app_eval_js` | Execute JavaScript in the app WebView |
| `app_screenshot` | Capture the app UI as text |
| `browser_navigate` | Navigate the in-app browser |
| `browser_click` | Click elements in the browser panel |
| `browser_screenshot` | Screenshot the browser panel |

REST shortcuts are also available (no MCP overhead):

```
GET  http://localhost:18091/api/state
POST http://localhost:18091/api/load-model   { "path": "..." }
POST http://localhost:18091/api/eval         { "js": "...", "target": "main" }
GET  http://localhost:18091/api/screenshot
GET  http://localhost:18091/api/browser/screenshot
POST http://localhost:18091/api/browser/click  { "selector": "..." }
```
