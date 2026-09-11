# 008 — Versioned config/agent migrations that never overwrite explicit user values

Status: INVESTIGATION — planning started 2026-09-11, nothing designed or implemented yet
Source: comparative review of `E:\repo\Atomic-Chat` (HEAD `64758ea`)

## Goal

Be able to fix an already-saved agent or config when our defaults change, exactly once,
without clobbering a value the user chose deliberately.

## Why this surfaced — we have a live instance of the gap

The `Qwen 3.8 27B` agent is stored with `context_size = 262144`. Confirmed today against
the running server:

```
Qwen 3.8 27B | ctx 262144 | turbo2 | turbo3   <- modelPresets.ts says 32768
Qwen 9B      | ctx  32768
```

That value dates from when the KV cost model under-counted by ~1.8× (see
[[007_backend_capability_probing]]) and it is what produced the original ~0 tok/s hang in
[[003_vram_fit_not_applied_or_warned]]. Tasks 003 (a) and (b) fixed the path for **new**
configs; the saved agent was deliberately left alone because we have no mechanism to change
a stored user value safely, and the user chose "leave it" when offered a one-off edit.

So the defect is not the 27B row — it is that "fix this for existing users" is currently
either *nothing* or *a manual edit*.

## Verified in the reference project

- Migrations are keyed and versioned, e.g. `llamacpp_kv_cache_migrated_turbo3_v2`
  (`extensions/llamacpp-upstream-extension/src/index.ts:930`), with an explicit list of
  keys to migrate (`const keysToMigrate = ['cache_type_k', 'cache_type_v']`).
- Earlier migrations are retained and **deliberately skipped** by the provider they don't
  apply to: *"NOTE: v2 turbo3 KV-cache migration is intentionally skipped for the … turboquant
  KV types"* — i.e. migrations are scoped by provider, not applied globally.
- Most valuable: the code records the scar from getting this wrong (`index.ts:566`):
  > "The one-shot 'migrate to turbo3' that lived here overwrote an explicit f16 / q8_0
  > choice as well as the default"

  That is precisely the failure we would have shipped if we "just fixed" the 27B row.
- ADR titles confirm this is routine for them: *"Migrate the stale macOS `llamacpp` default
  to `llamacpp-upstream` for pre-ATO-116 profiles"*, *"Windows: fix clean-install config
  persistence"*.

## The hard part

Distinguishing **"this is the default we shipped"** from **"the user typed this"**. Our
`agents` / `config` rows store final values only; there is no provenance flag. Without
provenance a migration cannot tell 262144-because-we-computed-it-wrong from
262144-because-the-user-wants-it.

Candidate approaches, none yet evaluated:
- Add a `source` column / JSON field per tuned field (`default` | `preset` | `user`).
- Store the preset name + version an agent was created from, and only migrate rows whose
  values still equal that preset's old values (i.e. untouched).
- Migrate nothing; instead **flag** stale rows in the UI and let the user apply the fix —
  the "won't fit" warning added in 003 (c) already does exactly this for the 27B.

The third is cheapest and is arguably already shipped. Worth deciding whether automatic
migration is wanted at all before building provenance tracking.

## Not yet verified

- Whether `schema_migrations` (already in `crates/llama-chat-db/src/schema/sql.rs`) is a
  suitable home for *data* migrations, or whether it should stay schema-only.
- How many saved agents/configs actually exist in the wild beyond this machine — if the
  answer is "one developer", automatic migration is over-engineering.

## Concrete next steps

- [ ] Decide the question above: **auto-migrate, or surface-and-let-the-user-apply?**
      Everything else depends on it.
- [ ] If auto-migrate: design provenance before writing any migration. A migration without
      provenance will overwrite intentional values — the reference project already made
      that mistake for us to learn from.
- [ ] Either way: write down the rule that a migration may change a *default* but never an
      *explicit* value.
