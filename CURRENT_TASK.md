# Agent-Based Config Migration

## Goal

Replace the current `config` (global singleton) + `conversation_config` (per-conversation snapshot)
architecture with a named **agent** system. Users create reusable agents that bundle all model/sampler/
hardware config. Conversations reference an agent by ID. `conversation_config` table is dropped entirely.

---

## New Schema

### `agents` table (NEW)
```sql
CREATE TABLE agents (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    -- provider
    provider_id TEXT NOT NULL DEFAULT 'local',  -- 'local' | 'anthropic' | 'groq' | etc.
    model_path TEXT,            -- local .gguf path (NULL for remote providers)
    provider_model TEXT,        -- remote model ID (NULL for local)
    -- system prompt
    system_prompt TEXT,
    system_prompt_type TEXT DEFAULT 'Default',
    -- sampler + hardware params (JSON blobs — adding new params = no migration needed)
    sampler_params TEXT,        -- {"temp": 0.8, "top_p": 0.95, "top_k": 40, ...}
    hardware_params TEXT,       -- {"n_gpu_layers": 99, "context_size": 32768, ...}
    -- tool config
    tool_config TEXT,           -- {"tag_pairs": [...], "tool_tag_exec_open": ..., ...}
    -- heartbeat (agent-level defaults, can be overridden per-conversation)
    heartbeat_enabled INTEGER DEFAULT 0,
    heartbeat_interval_minutes INTEGER DEFAULT 30,
    heartbeat_prompt TEXT,
    -- meta
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
```

### `conversations` table changes
Add columns, drop `conversation_config` dependency:
```sql
ALTER TABLE conversations ADD COLUMN agent_id TEXT REFERENCES agents(id);
ALTER TABLE conversations ADD COLUMN overrides TEXT;         -- sparse JSON deltas from agent
ALTER TABLE conversations ADD COLUMN heartbeat_enabled INTEGER DEFAULT 0;
ALTER TABLE conversations ADD COLUMN heartbeat_interval_minutes INTEGER DEFAULT 30;
ALTER TABLE conversations ADD COLUMN heartbeat_prompt TEXT;
ALTER TABLE conversations ADD COLUMN heartbeat_last_fired_at INTEGER DEFAULT 0;
ALTER TABLE conversations ADD COLUMN heartbeat_last_result TEXT;
ALTER TABLE conversations ADD COLUMN heartbeat_has_unread INTEGER DEFAULT 0;
```

### `conversation_config` table → DROPPED
All config now lives in `agents`. Per-conversation tweaks go into `conversations.overrides` (sparse JSON).
Migration: for each existing `conversation_config` row, create an anonymous agent, set `conversations.agent_id`.

### `config` table — stays, scope reduced
Keeps only app-level settings (no sampler/model params):
- UI prefs, telegram tokens, browser backend, models_directory
- `provider_api_keys`, `max_tool_calls`, `loop_detection_limit`
- `disable_file_logging`, `use_rtk`, `use_htmd`
- `web_browser_backend`, `models_directory`
- `active_provider` / `active_provider_model` → replaced by "last used agent" pointer

---

## Active Config Resolution

When a conversation is active, effective config = agent params merged with `overrides`:

```
agent.sampler_params (JSON)
  + agent.hardware_params (JSON)
  + conversations.overrides (sparse JSON, wins on conflict)
= effective ConversationConfig
```

Most conversations will have `overrides = NULL` (pure agent config).

---

## UI Changes

### Current: "Select provider" button
### New: "Select agent" button

**Agent selector modal:**
- List of created agents (name + model/provider badge + "Load" button)
- "+ New Agent" button → agent creation form
- "Edit" button per agent row

**Agent creation form:**
- Name (text input)
- Provider toggle: Local / Remote
  - Local: .gguf file picker + n_gpu_layers, context_size, cache types, flash_attention, etc.
  - Remote: provider dropdown + model picker + optional API key override
- System prompt (textarea) + type selector
- Sampler params (temp, top_p, top_k, repeat_penalty, DRY, etc.)
- Tool config (tag pairs, enabled tools)
- Heartbeat settings
- [Save] → back to agent list, new agent auto-highlighted

**Switching agents on a conversation:**
- Select different agent from the selector
- Unload current model / disconnect current provider
- Load new agent's model/provider
- Merge `conversations.overrides` stays (user-set tweaks survive agent switch)
- Message history stays intact

**Per-conversation overrides:**
- Settings panel still lets user tweak individual params
- Changes write to `conversations.overrides` (sparse JSON), not a separate table

---

## Migration Plan (existing DBs)

1. Create `agents` table
2. For each distinct `conversation_config` row, create an agent row:
   - name = "Imported agent" + conversation title (or "Default Agent" if all rows are identical)
   - params hydrated from `conversation_config` columns
3. Set `conversations.agent_id` → newly created agent IDs
4. Copy heartbeat columns from `conversation_config` → `conversations`
5. Drop `conversation_config` table
6. Strip sampler/model params from `config` table (DROP COLUMN for each)

---

## Implementation Order

### Step 1 — DB layer
- [ ] Add `agents` table to `sql.rs` + `schema.rs`
- [ ] Add `agent_id`, `overrides`, heartbeat columns to `conversations`
- [ ] Write `AgentRecord` Rust struct + CRUD methods in `llama-chat-db`
- [ ] Write config resolution fn: `resolve_conversation_config(agent, overrides) -> ConversationConfig`
- [ ] Write migration: existing `conversation_config` rows → anonymous agents

### Step 2 — Backend API
- [ ] `GET /agents` — list agents
- [ ] `POST /agents` — create agent
- [ ] `PUT /agents/:id` — update agent
- [ ] `DELETE /agents/:id` — delete agent
- [ ] `POST /conversations/:id/agent` — switch agent on conversation (triggers model reload)
- [ ] `PATCH /conversations/:id/overrides` — update per-conversation overrides

### Step 3 — Wire config resolution
- [ ] Replace `load_conversation_config()` reads with `resolve_conversation_config()`
- [ ] Worker: on agent switch, unload + reload with new agent params
- [ ] `save_conversation_config()` writes → write to `conversations.overrides` (sparse)

### Step 4 — Frontend
- [ ] Replace "Select provider/model" modal with Agent selector modal
- [ ] Agent creation form (local + remote paths)
- [ ] Edit agent flow
- [ ] Per-conversation overrides panel (writes to overrides JSON)
- [ ] Show active agent name in conversation header

### Step 5 — Cleanup
- [ ] Drop `conversation_config` table (after migration verified)
- [ ] Remove `ConversationConfig` Rust struct (replace with `AgentConfig` + `overrides` merge)
- [ ] Strip model/sampler columns from `config` table

---

## Notes

- **Don't lock agent to conversation** — user can switch agent mid-conversation. History survives, config resets to new agent baseline (overrides preserved).
- **Multiple simultaneous agents** already works — each conversation has its own worker process.
- **Backward compat** — existing DBs without `agents` table get auto-migrated on first launch.
- **`config` table** stays as app-level settings only; no sampler params.
