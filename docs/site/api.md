# REST API

The backend exposes a REST API on **http://localhost:18080** (web server mode) or via Tauri IPC (desktop mode).

The full machine-readable spec is at [`docs/openapi.json`](../openapi.json) (generated).

---

## Authentication

None — the server binds to localhost only.

---

## Core endpoints

### Models

```
GET    /api/models              List available local models
GET    /api/models/status       Current loaded model status
POST   /api/models/load         Load a model
POST   /api/models/unload       Unload the current model
```

### Chat

```
POST   /api/chat                Send a message, stream tokens via SSE
GET    /api/chat/status         Current generation status
POST   /api/chat/stop           Cancel ongoing generation
```

### Conversations

```
GET    /api/conversations           List all conversations
GET    /api/conversations/:id       Get a conversation with all messages
POST   /api/conversations           Create a new conversation
DELETE /api/conversations/:id       Delete a conversation
PATCH  /api/conversations/:id       Update title
GET    /api/conversations/:id/watch Watch for updates (SSE)
```

### Configuration

```
GET    /api/config              Get current app config
PUT    /api/config              Update app config
GET    /api/config/providers    List configured cloud providers
POST   /api/config/providers    Add/update a provider
DELETE /api/config/providers/:name  Remove a provider
```

### System / health

```
GET    /api/health              Server health + version
GET    /api/status              Worker status (model loaded, generating, VRAM)
GET    /api/system              System info (CPU, RAM, GPU)
```

---

## Chat streaming

`POST /api/chat` returns a Server-Sent Events stream:

**Request body:**
```json
{
  "conversation_id": "chat_2026-01-01-12-00-00-000",
  "message": "What is the capital of France?",
  "tools_enabled": false
}
```

**SSE event types:**
```
data: {"type":"token","content":"Paris"}
data: {"type":"token","content":" is"}
data: {"type":"stats","tokens_used":42,"max_tokens":8192}
data: {"type":"done","content":"Paris is the capital of France."}
data: {"type":"error","message":"..."}
```

---

## Load model

```
POST /api/models/load
Content-Type: application/json

{
  "model_path": "/path/to/model.gguf",
  "gpu_layers": 99,
  "context_size": 32768,
  "cache_type_k": "q8_0",
  "cache_type_v": "q8_0"
}
```

---

## WebSocket conversation watcher

```
WS /api/conversations/:id/ws
```

Pushes streaming token events and completion events in real time. Same event format as SSE chat.
