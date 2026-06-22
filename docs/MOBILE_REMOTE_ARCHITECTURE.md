# Desktop ↔ Mobile Architecture

## Overview

Allow a mobile device to control and monitor the desktop AI agent application while keeping all inference, tool execution, and filesystem access on the desktop.

The desktop is already the full backend (`llama_chat_web` binary on port 18080). The mobile device is a remote client consuming the same HTTP + WebSocket API already served to the local React frontend.

---

## High-Level Architecture

```
┌──────────────────────────────────────────────────┐
│ Desktop (llama_chat_web binary)                  │
│                                                  │
│  Axum HTTP server        :18080                  │
│  WebSocket server        ws://…:18080/ws/*       │
│  Worker process          (JSON Lines IPC)        │
│  Local LLM + tool executor                       │
│  SQLite (llama_chat_db)                          │
│  Agent runtime (WorkerPool + Agents)             │
│  React frontend          :14000 (dev) / static   │
└──────────────────────┬───────────────────────────┘
                       │ Encrypted tunnel
                       │ (Tailscale recommended)
              ┌────────▼────────┐
              │ Pairing Service │  (optional – QR only)
              │ (tiny cloud)    │
              └────────┬────────┘
                       │
              ┌────────▼────────┐
              │ Mobile Browser  │
              │ (responsive web)│
              └─────────────────┘
```

---

## Design Principles

- Desktop performs all LLM inference and tool execution.
- Desktop has exclusive filesystem access.
- Mobile is a read/write UI — it can send messages, approve actions, and monitor progress.
- Conversations and agent state remain on desktop SQLite.
- Cloud infrastructure is minimal (pairing ceremony only; no conversation data in cloud).

---

## What Already Exists

The desktop binary already exposes everything the mobile UI needs. No new server code is required — only auth and transport need to be added.

### HTTP API (Axum, port 18080)

```
GET  /health
GET  /api/info
GET  /api/conversations
POST /api/conversations
GET  /api/conversation/:id
DELETE /api/conversations/:id

POST /api/chat
POST /api/chat/stream          (SSE)
POST /api/chat/cancel

GET  /api/agents
POST /api/agents
GET  /api/agents/:id
PUT  /api/agents/:id
DELETE /api/agents/:id
POST /api/agents/:id/activate
POST /api/agents/:id/stop
GET  /api/agents/statuses

GET  /api/model/status
POST /api/model/load
POST /api/model/unload
GET  /api/backends

GET  /api/tools/available
POST /api/tools/execute

GET  /api/providers
POST /api/config/active-provider

/v1/chat/completions           (OpenAI-compatible)
/v1/models
```

### WebSocket Endpoints

```
ws://…/ws/chat/stream              real-time token streaming
ws://…/ws/conversation/watch/:id   watch a conversation for updates
ws://…/ws/status                   model load/status events
```

**Events emitted over `/ws/chat/stream`:**

| `type`             | Description                              |
|--------------------|------------------------------------------|
| `token`            | Streamed token chunk (batched ~40ms)     |
| `done`             | Generation complete + stats              |
| `error`            | Generation or tool error                 |
| `abort`            | Generation cancelled by user             |
| `heartbeat`        | Keep-alive ping (every 5s)               |
| `server_continuing`| Auto-continue after context overflow     |
| `status`           | In-progress status message (compaction)  |
| `tool_timing`      | Live tool call timing data               |

**Events emitted over `/ws/status`:**

| `type`         | Description            |
|----------------|------------------------|
| `model_status` | Current model load state |
| `update`       | Worker status update   |

### Worker Architecture

The HTTP server spawns an out-of-process worker (`--worker` flag) and communicates via JSON Lines over stdin/stdout (`WorkerCommand` / `WorkerPayload` in `llama-chat-types/src/ipc_types.rs`). The worker owns the GGUF model and GPU context. Multiple workers are supported via `WorkerPool`.

---

## What Needs to Be Added

### 1. Authentication

The API is currently unauthenticated. Remote access requires a Bearer token on every request.

**Recommended: Ed25519 key pair per device**

```
First launch → generate desktop Ed25519 key pair
Pairing     → phone generates its own key pair
             → phone sends public key to desktop during QR flow
             → desktop issues a signed JWT / HMAC token
             → phone includes token in every request:
               Authorization: Bearer <token>
```

Axum middleware (tower layer) validates the token before any route handler runs.

**Revocation**: store issued device tokens in SQLite `remote_devices` table; delete row to revoke.

### 2. QR Pairing

Avoids exposing the raw IP/hostname in the QR code for LAN-only scenarios.

**Simple LAN flow (no cloud service needed):**

1. Desktop generates a short-lived pairing code (e.g. `X7M92K`, expires 5 min).
2. Desktop shows QR containing `{ "host": "<tailscale-hostname>", "code": "X7M92K" }`.
3. Phone scans QR and POSTs `{ code, phone_public_key }` to `POST /api/pair`.
4. Desktop UI shows "Accept pairing from phone?" — user taps Yes.
5. Desktop returns a long-lived token + its own public key.
6. Phone stores token; future connections go direct — no QR needed again.

**With relay (if Tailscale isn't used):**

Same flow, but step 2 embeds the relay service URL instead of direct hostname. Desktop and phone both hold a WebSocket to the relay; relay forwards encrypted blobs only.

### 3. Remote Connectivity

**Option A — Tailscale (recommended)**

Desktop installs Tailscale and gets a stable `100.x.x.x` / `<hostname>.ts.net` address. Mobile browser connects directly over the private network. No ports opened to the public internet, no relay required.

QR code: `{ "host": "mydesktop.tailnet.ts.net", "code": "X7M92K" }`

**Option B — Relay Service**

If users won't install Tailscale. Deploy a small relay (e.g. on Fly.io):

```
Phone → WebSocket → Relay → WebSocket → Desktop
```

The relay never decrypts payloads (E2E encrypted with NaCl box / AEAD). Relay stores only:
- `pairings(device_id, desktop_id, created_at)`
- `device_presence(device_id, last_seen)`

No conversation data, no files, no prompts.

### 4. Approval Workflow

Agent tool execution already runs on desktop. The mobile UI needs a way to intercept and approve dangerous actions before execution.

**Proposed flow:**

1. Model wants to run `execute_command("git push origin main")`.
2. Server emits `approval_required` event over the active WebSocket connection.
3. All connected clients (desktop + mobile) receive it.
4. First client to respond wins: `POST /api/conversations/:id/approve` or `/reject`.
5. Desktop resumes or cancels the tool call.

```json
{ "type": "approval_required", "id": "req-123", "action": "execute_command", "args": { "command": "git push origin main" } }
```

Required for (configurable per-agent):
- `execute_command` (shell execution)
- `write_file` on paths outside the project root
- `git push`, destructive git operations
- External network access via `web_fetch`

This maps naturally onto the existing agent heartbeat system (`/api/conversations/:id/heartbeat`).

---

## Mobile Client

No dedicated native app is needed initially. The existing React frontend is already responsive.

**Local**: `http://localhost:18080`  
**Remote**: `https://<hostname>.ts.net` (Tailscale) or relay URL

The UI already handles:
- Chat input and streaming token display
- Agent activation / monitoring
- Model status
- Conversation history

Mobile-specific additions worth prioritizing:
- Compact layout for small screens (already partially there via Tailwind responsive classes)
- Approve/Reject action sheet for the approval workflow
- Push notification when the agent pauses and needs approval

For push notifications, a PWA service worker can receive Web Push from the relay or Tailscale-reachable desktop.

Later the web app can be wrapped with Android WebView or iOS WKWebView with zero code changes.

---

## Security Model

Never expose execution endpoints without authentication.

| Endpoint category | Risk | Control |
|---|---|---|
| `POST /api/chat` | Model sees messages | Auth token required |
| `POST /api/tools/execute` | Shell execution | Auth + approval workflow |
| `POST /api/model/load` | Loads arbitrary GGUF | Auth required |
| `DELETE /api/conversations/…` | Data loss | Auth required |
| `GET /api/conversations` | Conversation history | Auth required |

The auth middleware should be added as a tower layer wrapping the entire Axum router, exempting only:
- `GET /health` (monitoring)
- `POST /api/pair` (pairing ceremony — validated by short-lived code, not token)
- Static asset serving

---

## SQLite Schema Additions

Existing tables (in `llama_chat_db`): conversations, messages, agents, downloads, mcp_servers, config, background_processes, app_errors.

New tables needed:

```sql
CREATE TABLE remote_devices (
    id          TEXT PRIMARY KEY,      -- UUID
    name        TEXT NOT NULL,         -- "Agus iPhone"
    public_key  BLOB NOT NULL,         -- Ed25519 public key bytes
    token_hash  TEXT NOT NULL,         -- SHA-256 of issued token
    created_at  INTEGER NOT NULL,
    last_seen   INTEGER
);

CREATE TABLE pairing_codes (
    code        TEXT PRIMARY KEY,      -- 6-char alphanumeric
    expires_at  INTEGER NOT NULL,
    used        INTEGER NOT NULL DEFAULT 0
);
```

---

## Minimum Viable Path

1. **Add Bearer token auth** — Axum middleware, hardcoded single token stored in SQLite config (simplest possible start).
2. **QR code on desktop UI** — encode `{ host, token }` directly (no relay, LAN-only via Tailscale).
3. **Test mobile browser** — open `http://<tailscale-host>:18080` on phone; should work with existing React UI.
4. **Add approval_required WS event** — intercept dangerous tool calls; mobile UI shows approve/reject.
5. **Upgrade to per-device keys** — replace single token with Ed25519 key pairs + proper pairing flow.
6. **Relay service** — only if users won't use Tailscale.

---

## Future Enhancements

- Web Push notifications (agent paused, generation complete)
- Multi-device support (tablet + phone simultaneously)
- Session sync (resume from phone mid-conversation started on desktop)
- Offline task history view (cached on phone)
- Native Android/iOS wrappers (WKWebView / WebView)
- End-to-end encrypted relay
- Fine-grained per-agent approval policy (which tool categories require approval)
