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

## Implementation Status

### Step 1 — DB layer
- [x] Add `agents` table to `sql.rs` + `schema.rs`
- [x] Add `agent_id`, `overrides` columns to `conversations`
- [x] Write `AgentRecord` Rust struct + CRUD methods in `llama-chat-db`
- [x] Config resolution fn: `load_effective_config()` in `agents.rs` (agent → conversation_config → global fallback)
- [x] Migration: existing `conversation_config` rows → anonymous agents

### Step 2 — Backend API
- [x] `GET /api/agents`
- [x] `POST /api/agents`
- [x] `PUT /api/agents/:id`
- [x] `DELETE /api/agents/:id`
- [x] `POST /conversations/:id/agent`
- [x] `PATCH /conversations/:id/overrides`
- [x] **Wire routes into HTTP router** — all agent routes dispatched in `src/main_web.rs` (lines 351–379)

### Step 3 — Wire config resolution
- [x] **Call `load_effective_config()` in the engine** — this is the core of the whole migration: without it,
  an agent's system_prompt and sampler params are saved to DB but never actually used during generation;
  the engine still reads the old global config
- [x] **Worker: reload on agent switch** — `POST /conversations/:id/agent` saves the agent_id but doesn't
  trigger a model unload/reload, so switching agents has no effect on the running inference
- [x] Sparse overrides stored and merged (`set_conversation_overrides` + `apply_overrides`)

### Step 4 — Frontend
- [x] Agent selector modal (create, edit, delete, local/remote/CLI)
- [x] Agent creation + edit form
- [x] **Fix `apiClient` import** — `AgentContext.tsx` imports from `../utils/apiClient` which was deleted
  in the last refactor; the agent UI is broken until this is replaced with direct `fetch` calls
- [x] Per-conversation overrides panel — lets users tweak individual params per conversation without
  editing the agent; writes sparse JSON to `conversations.overrides`
- [x] Show active agent name in chat header — user feedback so they know which agent is loaded

### Step 5 — Cleanup (after steps 2–4 verified working)
- [x] Drop `conversation_config` table
- [x] Remove `ConversationConfig` Rust struct
- [x] Strip model/sampler columns from `config` table

---

## Notes

- **Don't lock agent to conversation** — user can switch agent mid-conversation. History survives, config resets to new agent baseline (overrides preserved).
- **Multiple simultaneous agents** already works — each conversation has its own worker process.
- **Backward compat** — existing DBs without `agents` table get auto-migrated on first launch.
- **`config` table** stays as app-level settings only; no sampler params.

---

# Code Quality Backlog

Improvements similar to the JSX ternary ban (add ESLint rule + fix all violations).

## High Impact

### 1. `react/no-unstable-nested-components` → error
Components defined inside render reset their state on every render (real correctness bug, not just style).
Currently `warn`. Upgrade to `error` and move inline component definitions outside their parent.
Example: `SectionHeader` was defined inside `ProviderSelector` render body.

### 2. `@typescript-eslint/no-explicit-any` → error
Currently `warn`. Forces `unknown` or proper types — catches real bugs at the type level.
~20-40 uses across the codebase to fix.

### 3. Ban `react/jsx-no-bind` (inline arrow functions on event handlers)
`onClick={() => doSomething(arg)}` creates a new function on every render.
Rule: `'react/jsx-no-bind': ['warn', { allowArrowFunctions: false }]`
Requires extracting to `useCallback` or named handlers defined outside JSX.

## Medium Impact

### 4. `no-nested-ternary` → error (everywhere)
We banned JSX ternaries but `no-nested-ternary` is still `warn` for regular JS.
Same principle: extract nested ternaries to variables or if/else blocks.

### 5. `jsx-a11y` rules → error
All accessibility rules are currently `warn`. Upgrading forces real structural fixes
(missing ARIA labels, keyboard nav, label associations). Relevant since the app
has desktop-tool and agent use cases where keyboard nav matters.

### 6. `import/no-default-export`
Most components already use named exports. Enforcing named-only improves
tree-shaking and makes imports predictable (no guessing the export name).

## Low Effort, Cleanup Value

### 7. `@typescript-eslint/consistent-type-imports` → error
Currently `warn`. Ensures `import type` is used for type-only imports — better
for build performance and avoids accidental value imports.

### 8. `no-console` → error
Currently allows `console.warn` and `console.error`. Strict enforcement removes
debug noise; all logging should go through a structured logger or be removed.
