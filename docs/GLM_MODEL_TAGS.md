# GLM-4.6V-Flash — Complete Tag Reference

Model: **GLM-4.6V-Flash** (Zai-org)
Architecture: `glm4` | Size: 9.4B | Context: 131,072 tokens
GGUF `general.name`: `Zai org_GLM 4.6V Flash`
Tokenizer: `gpt2` with `glm4` pre-tokenizer | Vocab: 151,552 tokens

## Special Token IDs

| ID | Token | Type | Description |
|----|-------|------|-------------|
| 151329 | `<\|endoftext\|>` | control (3) | BOS / EOS / PAD / UNK — serves all four roles |
| 151330 | `[MASK]` | control (3) | Mask token (MLM legacy) |
| 151331 | `[gMASK]` | control (3) | Generative mask — prepended before `<sop>` at conversation start |
| 151332 | `[sMASK]` | control (3) | Sentence mask (MLM legacy) |
| 151333 | `<sop>` | control (3) | Start of prompt — immediately after `[gMASK]` |
| 151334 | `<eop>` | control (3) | End of prompt |

## Role Markers

| ID | Token | Type | Description |
|----|-------|------|-------------|
| 151335 | `<\|system\|>` | control (3) | System message start |
| 151336 | `<\|user\|>` | control (3) | User turn start (also the `eot_token_id`) |
| 151337 | `<\|assistant\|>` | control (3) | Assistant turn start |
| 151338 | `<\|observation\|>` | control (3) | Tool result injection point — model expects results after this |

## Thinking Tags

| ID | Token | Type | Description |
|----|-------|------|-------------|
| 151350 | `<think>` | special (4) | Start of chain-of-thought reasoning block |
| 151351 | `</think>` | special (4) | End of reasoning block |
| 151360 | `/nothink` | control (3) | Appended to user message to suppress thinking (chat template checks this) |

## Tool Call Tags

| ID | Token | Type | Description |
|----|-------|------|-------------|
| 151352 | `<tool_call>` | special (4) | Start of a tool invocation |
| 151353 | `</tool_call>` | special (4) | End of a tool invocation |
| 151354 | `<tool_response>` | special (4) | Start of tool result content |
| 151355 | `</tool_response>` | special (4) | End of tool result content |
| 151356 | `<arg_key>` | special (4) | Argument key in GLM native XML format |
| 151357 | `</arg_key>` | special (4) | Close argument key tag |
| 151358 | `<arg_value>` | special (4) | Argument value in GLM native XML format |
| 151359 | `</arg_value>` | special (4) | Close argument value tag |

## Vision / Multimodal Tags

| ID | Token | Type | Description |
|----|-------|------|-------------|
| 151339 | `<\|begin_of_image\|>` | control (3) | Image embedding start marker |
| 151340 | `<\|end_of_image\|>` | control (3) | Image embedding end marker |
| 151363 | `<\|image\|>` | control (3) | Image placeholder (between begin/end) |
| 151341 | `<\|begin_of_video\|>` | control (3) | Video embedding start marker |
| 151342 | `<\|end_of_video\|>` | control (3) | Video embedding end marker |
| 151364 | `<\|video\|>` | control (3) | Video placeholder (between begin/end) |
| 151343 | `<\|begin_of_audio\|>` | control (3) | Audio start marker (unused in this model) |
| 151344 | `<\|end_of_audio\|>` | control (3) | Audio end marker (unused in this model) |
| 151345 | `<\|begin_of_transcription\|>` | control (3) | Transcription start |
| 151346 | `<\|end_of_transcription\|>` | control (3) | Transcription end |
| 151361 | `<\|begin_of_box\|>` | control (3) | Vision bounding box / spatial grounding start |
| 151362 | `<\|end_of_box\|>` | control (3) | Vision bounding box end |

## Code FIM Tags

| ID | Token | Type | Description |
|----|-------|------|-------------|
| 151347 | `<\|code_prefix\|>` | control (3) | Fill-in-the-middle: prefix section |
| 151348 | `<\|code_middle\|>` | control (3) | FIM: cursor / hole location |
| 151349 | `<\|code_suffix\|>` | control (3) | FIM: suffix section |

## Pad Tokens

| ID Range | Count | Description |
|----------|-------|-------------|
| 151365–151551 | 187 | `[PADnnnnn]` reserved pad tokens (type=5) |

---

## Chat Template — Native Conversation Format

The GGUF-embedded Jinja2 template defines the expected format:

```
[gMASK]<sop><|system|>
{system_prompt}
<|user|>
{user_message}
<|assistant|>
<think>{reasoning}</think>
{response_text}
<tool_call>{function_name}
<arg_key>{key}</arg_key>
<arg_value>{value}</arg_value>
</tool_call>
<|observation|>
<tool_response>
{tool_output}
</tool_response>
<|assistant|>
...continues...
```

### Key Chat Template Behaviors

1. **Thinking**: Every assistant turn gets `<think>...</think>` — even empty `<think></think>` when thinking is disabled
2. **Tool calls use XML format natively**: `<tool_call>name\n<arg_key>k</arg_key>\n<arg_value>v</arg_value>\n</tool_call>`
3. **Tool results go under `<|observation|>`**: The observation role marker precedes `<tool_response>` blocks
4. **Multiple tool results**: Consecutive `tool` role messages share a single `<|observation|>` tag
5. **No explicit turn-end token**: Turns are delimited by the next role marker, not a closing tag
6. **`/nothink` suppression**: If the user message ends with `/nothink`, thinking is bypassed

---

## Our Handling — What We Do vs. What the Model Expects

### Stop Tokens (generation.rs / models.rs)

| Token | Configured as stop? | Notes |
|-------|---------------------|-------|
| `<\|endoftext\|>` | Yes (EOS) | GGUF `eos_token_id` |
| `<\|user\|>` | Yes | `get_common_stop_tokens()` — prevents hallucinated user turns |
| `<\|observation\|>` | Yes | `get_common_stop_tokens()` — marks tool result boundary |
| `<\|system\|>` | Yes | `get_common_stop_tokens()` — prevents hallucinated system turns |
| `<\|assistant\|>` | Yes | `get_common_stop_tokens()` — prevents hallucinated assistant turns |
| `<\|end_of_box\|>` | No (stripped in display) | Vision bounding box — not a generation stop |
| `</tool_call>` | Handled by ExecBlockTracker | Triggers tool execution, not a hard stop |
| `</think>` | No | Thinking end — generation continues after |
| `<eop>` | No | Not typically generated |

### Tool Call Format (templates.rs)

| Aspect | What model expects (chat template) | What we instruct (system prompt) |
|--------|-------------------------------------|----------------------------------|
| Wrapper tags | `<tool_call>...</tool_call>` | `<tool_call>...</tool_call>` |
| Body format | XML: `name\n<arg_key>k</arg_key>\n<arg_value>v</arg_value>` | JSON: `{"name":"...","arguments":{...}}` |
| Tool list | `<tools>` block with JSON schemas | Markdown ### sections with examples |
| Result wrapper | `<|observation|>\n<tool_response>...</tool_response>` | `<tool_response>...</tool_response>` (no `<|observation|>`) |
| Multiple calls | One `<tool_call>` per call | JSON array inside single `<tool_call>` |

### Tag Stripping (frontend display)

| Tag | Stripped? | Where |
|-----|-----------|-------|
| `<think>...</think>` | Extracted to thinking panel | `useMessageParsing.ts` |
| `<tool_call>...</tool_call>` | Rendered as tool call widget | `toolSpanCollectors.ts` |
| `<tool_response>...</tool_response>` | Rendered as tool output in widget | `toolSpanCollectors.ts` |
| `<\|begin_of_image\|>`, `<\|image\|>`, `<\|end_of_image\|>` | Stripped | `GLM_VISION_CLEANUP` regex |
| `<\|begin_of_video\|>`, `<\|video\|>`, `<\|end_of_video\|>` | Stripped | `GLM_VISION_CLEANUP` regex |
| `<\|begin_of_box\|>`, `<\|end_of_box\|>` | Stripped | `GLM_VISION_CLEANUP` regex |
| `<arg_key>...</arg_key>` | Parsed as tool args (fallback) | `toolParser.ts`, `native_tools.rs` |
| `<arg_value>...</arg_value>` | Parsed as tool args (fallback) | `toolParser.ts`, `native_tools.rs` |

---

## Known Gaps / Issues

1. **No `<|observation|>` in result injection**: Our system wraps tool output in `<tool_response>...</tool_response>` but does NOT prepend `<|observation|>`. The chat template expects it. This may reduce the model's ability to distinguish tool results from regular text.

2. **JSON vs XML body mismatch**: We instruct JSON format but the model was trained on XML `<arg_key>`/`<arg_value>`. The model usually follows our JSON instruction, but sometimes falls back to native XML. We have fallback parsers for this, but the mismatch means extra tokens are wasted on format confusion.

3. **Parallel tool calls**: We instruct JSON arrays `[{...},{...}]` inside a single `<tool_call>` block. The native format uses separate `<tool_call>` blocks for each call. Model behavior with our array format is untested.

4. **Tool list format**: We provide markdown tool descriptions. The chat template expects `<tools>` XML block with JSON schema definitions. This is a significant divergence from training data.

5. **Thinking control**: We don't append `/nothink` to user messages or manage `<think></think>` insertion per the template. The model generates thinking blocks on its own, which works, but we're not using the template's explicit control mechanism.

6. **`<eop>` never configured**: End-of-prompt token exists but is never used as a stop token. Unclear if the model ever generates it.

7. **`<|end_of_box|>` as tool_call close**: GLM sometimes emits `<|end_of_box|>` instead of `</tool_call>` to close tool calls. We handle this in `stripUnclosedToolCallTail` and `ExecBlockTracker`, but it's a symptom of format confusion — the model is mixing bounding box and tool call semantics.

---

## Planned Refactor: Native Format Tool System (Big Rewrite)

### The Core Problem

**Models don't reliably follow system prompt instructions about tool format.** The current approach fights the model's training:

- We tell GLM to use JSON: `<tool_call>{"name":"read_file","arguments":{"path":"x"}}`
- GLM was **trained** on XML: `<tool_call>read_file\n<arg_key>path</arg_key>\n<arg_value>x</arg_value></tool_call>`
- Sometimes the model complies with our JSON instruction, sometimes it falls back to its native XML
- We then need fallback parsers in ~5 files to catch both formats
- Same problem across all model families — each has its own native format we're overriding

**The fix: stop fighting the models. Adapt the entire pipeline to each model's native format.**

### What "Big Rewrite" Means

This is NOT just adding a UI for tag pairs. The tag pairs define **how the entire tool pipeline works per model**:

| Pipeline Stage | Currently | After Rewrite |
|----------------|-----------|---------------|
| **System prompt** | Universal JSON format for all models | Native format examples per model (GLM gets XML, Mistral gets bracket) |
| **Tool call parsing** | Universal JSON parser + fallback parsers for native formats | Primary parser uses model's native format directly |
| **Tool result injection** | `<tool_response>output</tool_response>` for all | GLM: `<\|observation\|>\n<tool_response>output</tool_response>`, Mistral: `[TOOL_RESULTS]output[/TOOL_RESULTS]` |
| **Stop tokens** | Hardcoded list in `get_common_stop_tokens()` | Auto-derived from role marker tag pairs |
| **Display stripping** | Hardcoded regex per model family (`GLM_VISION_CLEANUP`, `MISTRAL_CALL_CLEANUP`, etc.) | Driven by tag pairs list — strip all enabled pairs from display |
| **Thinking extraction** | Hardcoded `<think>`/`</think>` regex | Driven by thinking tag pair (some models use `<thinking>`/`</thinking>`) |
| **ExecBlockTracker** | Hardcoded exec_open/exec_close detection | Reads from tag pairs |

### Data Model

```typescript
interface TagPair {
  category: string;  // "tool" | "thinking" | "vision" | "role" | "control" | "code_fim" | "modifier"
  name: string;      // "exec" | "response" | "arg_key" | "arg_value" | "think" | "image" | etc.
  open_tag: string;  // "<tool_call>" | "<think>" | "<|user|>" etc.
  close_tag: string; // "</tool_call>" | "" (empty for single-token markers)
  enabled: boolean;
}
```

Additionally, each model preset needs a **tool call body format** descriptor:

```typescript
type ToolBodyFormat = "json" | "glm_xml" | "mistral_bracket" | "mistral_comma" | "llama3_xml";
```

This tells the system prompt generator and parser which format to use for the tool call body INSIDE the exec tags.

### GLM-4.6V-Flash Preset (17 tag pairs)

| Category | Name | Open Tag | Close Tag | Enabled |
|----------|------|----------|-----------|---------|
| tool | exec | `<tool_call>` | `</tool_call>` | yes |
| tool | response | `<tool_response>` | `</tool_response>` | yes |
| tool | arg_key | `<arg_key>` | `</arg_key>` | yes |
| tool | arg_value | `<arg_value>` | `</arg_value>` | yes |
| thinking | think | `<think>` | `</think>` | yes |
| vision | image | `<\|begin_of_image\|>` | `<\|end_of_image\|>` | yes |
| vision | video | `<\|begin_of_video\|>` | `<\|end_of_video\|>` | yes |
| vision | box | `<\|begin_of_box\|>` | `<\|end_of_box\|>` | yes |
| vision | audio | `<\|begin_of_audio\|>` | `<\|end_of_audio\|>` | yes |
| vision | transcription | `<\|begin_of_transcription\|>` | `<\|end_of_transcription\|>` | yes |
| role | system | `<\|system\|>` | | yes |
| role | user | `<\|user\|>` | | yes |
| role | assistant | `<\|assistant\|>` | | yes |
| role | observation | `<\|observation\|>` | | yes |
| control | eof | `<\|endoftext\|>` | | yes |
| control | sop | `<sop>` | | yes |
| modifier | nothink | `/nothink` | | yes |

**Body format:** `glm_xml`

### Per-Model Native Format Examples

#### GLM (body_format: `glm_xml`)

System prompt would teach:
```
<tool_call>read_file
<arg_key>path</arg_key>
<arg_value>filename.txt</arg_value>
</tool_call>
```

Result injection:
```
<|observation|>
<tool_response>
file contents here
</tool_response>
```

#### Mistral (body_format: `mistral_bracket`)

System prompt would teach:
```
[TOOL_CALLS]read_file[ARGS]{"path": "filename.txt"}
```

Result injection:
```
[TOOL_RESULTS]
file contents here
[/TOOL_RESULTS]
```

#### Qwen (body_format: `json`)

System prompt would teach:
```
<tool_call>{"name": "read_file", "arguments": {"path": "filename.txt"}}</tool_call>
```

Result injection:
```
<tool_response>
file contents here
</tool_response>
```

#### Default / Unknown (body_format: `json`)

Same as current system — JSON inside SYSTEM.EXEC tags.

### Storage

- Single `tag_pairs TEXT` column (JSON array) in both `config` and `conversation_config` tables
- Single `tool_body_format TEXT` column — the format string ("json", "glm_xml", etc.)
- Same pattern as existing `stop_tokens` column (JSON TEXT)
- Existing 4 `tool_tag_*` columns stay for backward compatibility during migration
- When `tag_pairs` is null/empty → fall back to existing 4-field system

### Rewrite Scope — Files Affected

#### Backend (Rust)

| File | Change | Scope |
|------|--------|-------|
| `src/web/chat/tool_tags.rs` | Add `TagPair` struct, full preset maps per model, `ToolBodyFormat` enum, `derive_tool_tags_from_pairs()` | Major |
| `src/web/chat/templates.rs` | **Rewrite `get_universal_system_prompt_with_tags()`** → `get_native_system_prompt(tags, body_format, tag_pairs)` — generates model-native tool call examples | Major |
| `src/web/chat/command_executor.rs` | **Rewrite tool call detection** — use tag pairs to build regex dynamically, parse body based on `tool_body_format` | Major |
| `src/web/chat/generation.rs` | Result injection uses tag pairs (observation marker, response wrapper) | Medium |
| `src/web/chat/stop_conditions.rs` | `ExecBlockTracker` reads exec tags from tag pairs, not hardcoded | Medium |
| `src/web/native_tools.rs` | **Rewrite dispatch** — primary parser matches `tool_body_format`, remove fallback chains | Major |
| `src/web/models.rs` | Add `tag_pairs`, `tool_body_format` to `SamplerConfig` + derive stop tokens from role pairs | Medium |
| `src/web/database/schema.rs` | Add `tag_pairs TEXT`, `tool_body_format TEXT` columns | Small |
| `src/web/database/config.rs` | Add fields to `DbSamplerConfig`, JSON serialize/deserialize | Small |
| `src/web/config.rs` | Map new fields in db↔sampler conversion | Small |
| `src/web/routes/model.rs` | Add `detected_tag_pairs` + `detected_body_format` to model metadata API | Small |

#### Frontend (TypeScript/React)

| File | Change | Scope |
|------|--------|-------|
| `src/types/index.ts` | Add `TagPair`, `ToolBodyFormat`, update `SamplerConfig` + `ModelMetadata` | Small |
| `src/config/modelPresets.ts` | Add `MODEL_TAG_PAIRS` + `MODEL_BODY_FORMATS` maps | Medium |
| NEW `src/components/organisms/model-config/TagPairsSection.tsx` | New component replacing `ToolTagsSection.tsx` | New |
| `src/components/organisms/model-config/index.tsx` | Replace `<ToolTagsSection>` with `<TagPairsSection>`, auto-populate on detect | Small |
| `src/utils/toolParser.ts` | **Rewrite `autoParseToolCalls()`** — use tag pairs to determine parser, not auto-detect chain | Major |
| `src/utils/toolSpanCollectors.ts` | **Rewrite segment builders** — use tag pairs for span detection, not hardcoded regex per format | Major |
| `src/utils/toolFormatUtils.ts` | `stripUnclosedToolCallTail()` — use tag pairs instead of hardcoded format checks | Medium |
| `src/hooks/useMessageParsing.ts` | Strip tags dynamically from tag pairs, not hardcoded `GLM_VISION_CLEANUP` etc. | Medium |
| `src/hooks/useConversationWatcher.ts` | `hasUnclosedToolExecution` — use tag pairs | Small |

### UI Mockup

```
┌─ Tag Pairs ──────────────────────────────────────────────┐
│ Body Format: [glm_xml ▼]  [Reset to Detected]  [+ Add]  │
│                                                          │
│ ▼ Tool (4)                                               │
│   exec       <tool_call>          </tool_call>     ☑ 🗑  │
│   response   <tool_response>      </tool_response> ☑ 🗑  │
│   arg_key    <arg_key>            </arg_key>       ☑ 🗑  │
│   arg_value  <arg_value>          </arg_value>     ☑ 🗑  │
│                                                          │
│ ▼ Thinking (1)                                           │
│   think      <think>              </think>         ☑ 🗑  │
│                                                          │
│ ▼ Vision (4)                                             │
│   image      <|begin_of_image|>   <|end_of_image|> ☑ 🗑  │
│   video      <|begin_of_video|>   <|end_of_video|> ☑ 🗑  │
│   box        <|begin_of_box|>     <|end_of_box|>   ☑ 🗑  │
│   audio      <|begin_of_audio|>   <|end_of_audio|> ☑ 🗑  │
│                                                          │
│ ▼ Role (4)                                               │
│   system     <|system|>                            ☑ 🗑  │
│   user       <|user|>                              ☑ 🗑  │
│   assistant  <|assistant|>                         ☑ 🗑  │
│   observation <|observation|>                      ☑ 🗑  │
│                                                          │
│ ► Control (2)  [collapsed]                               │
│ ► Modifier (1) [collapsed]                               │
└──────────────────────────────────────────────────────────┘
```

### Implementation Order

This is a big rewrite, so we break it into stages that each leave the system functional:

**Stage 1: Data model + storage + UI (no behavior change)**
- Add `TagPair` struct, presets, DB columns, API enrichment
- New `TagPairsSection` component in model config modal
- Pre-populate on model detect, persist to DB
- Existing pipeline unchanged — still uses old 4-field ToolTags

**Stage 2: System prompt rewrite**
- `templates.rs` generates native format examples based on `tool_body_format`
- GLM gets XML examples, Mistral gets bracket examples, Qwen gets JSON examples
- This alone should significantly improve model compliance

**Stage 3: Backend parser rewrite**
- `command_executor.rs` and `native_tools.rs` use `tool_body_format` as primary parser
- Remove fallback parser chains — each model gets ONE parser matched to its native format
- Result injection uses tag pairs (`<|observation|>` prefix for GLM, etc.)

**Stage 4: Frontend parser rewrite**
- `toolParser.ts` uses tag pairs to determine parser
- `toolSpanCollectors.ts` builds spans from tag pairs, not hardcoded regex
- `useMessageParsing.ts` strips tags dynamically
- Remove `GLM_VISION_CLEANUP`, `MISTRAL_CALL_CLEANUP`, etc.

**Stage 5: Stop token automation**
- Role marker tag pairs auto-populate stop tokens
- Remove `get_common_stop_tokens()` hardcoded list
- Stop tokens become a derived property of the tag pairs

### What Gets Deleted After Full Rewrite

- `GLM_VISION_CLEANUP` regex in `useMessageParsing.ts`
- `MISTRAL_CALL_CLEANUP` and `MISTRAL_RESULT_CLEANUP` regex
- `EXEC_CLEANUP` and `SYS_OUTPUT_CLEANUP` regex
- `try_parse_glm_xml_format()` in `native_tools.rs` (becomes the primary GLM parser)
- `try_parse_mistral_comma_format()` fallback chain
- `try_parse_llama3_xml_format()` fallback chain
- `parseMistralBracket/parseMistralClosedTag/parseMistralBareJson` auto-detect chain in `toolParser.ts`
- `collectMistralSpans/collectQwenSpans/collectLlama3Spans` hardcoded collectors in `toolSpanCollectors.ts`
- `get_common_stop_tokens()` hardcoded list in `models.rs`
- The 4 `tool_tag_*` columns (deprecated, eventually removed)
- `TOOL_TAG_FAMILIES` / `MODEL_TOOL_TAGS` in `modelPresets.ts` (replaced by `MODEL_TAG_PAIRS`)

### Jinja2 Chat Template — The Missing Piece

The GGUF-embedded Jinja2 template is the **source of truth** for how the model expects conversations to be formatted. Our `templates.rs` manually reimplements this per model family (ChatML, Mistral, Llama3, GLM, etc.) but doesn't match exactly — this is a major source of compliance issues.

#### Three Integration Options

| Option | How | Fidelity | Complexity |
|--------|-----|----------|------------|
| **llama.cpp built-in** | `LlamaModel::apply_chat_template()` — already wrapped in Rust bindings | Medium — supports ~50 predefined templates, NOT a real Jinja2 parser | Low — API already available |
| **minijinja crate** | Real Jinja2 renderer — handles ANY template including custom ones | High — exact match to model training data | Medium — new dependency |
| **Manual (current)** | `templates.rs` hardcoded per template type | Low — approximation, not exact | Already done |

#### What We Already Have

```rust
// In llama-cpp-2 (our Rust bindings):
model.apply_chat_template(&template, &messages, add_assistant_prompt)  // formats conversations
model.chat_template(None)  // retrieves template from GGUF

// In our code:
LlamaState.chat_template_string: Option<String>  // already stored on model load
```

#### Relationship to Tag Pairs

Tag pairs and the Jinja2 template are **complementary**:
- **Tag pairs** define WHAT tags exist → drives parsing, display, config UI
- **Jinja2 template** defines HOW to format conversations using those tags → drives prompt construction

Future stage: replace `templates.rs` with `llama_chat_apply_template()` or `minijinja`. The model's own template formats the prompt; the tag pairs drive the parsing/display side.

### Open Questions

1. **How to handle unknown models?** — Default to JSON body format + SYSTEM.EXEC tags (current behavior). User can manually configure tag pairs in the UI if they know their model's format.
2. **Should we try to auto-detect body format from GGUF chat template?** — The Jinja2 template contains format clues (e.g., `<arg_key>` presence → `glm_xml`). Could parse this automatically instead of hardcoding per model name.
3. **What about the Harmony model?** — It uses a completely different turn-level format (`to=tool_name code<|message|>{JSON}<|call|>`). Does it fit the tag pair model or does it need its own special case?
4. **Parallel tool calls** — Different models handle multiple tool calls differently (array vs separate blocks). Should `tool_body_format` also encode this, or is it a separate config flag?
5. **Jinja2 integration stage** — When do we switch from manual `templates.rs` to using the model's native template? This could be a separate stage after tag pairs are working.
