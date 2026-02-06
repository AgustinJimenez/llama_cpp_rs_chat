# Tool System Redesign - Universal Command Execution

## Final Design Decision

**Token Format:** `<||SYSTEM.EXEC>command<SYSTEM.EXEC||>`
- Uses `||` pipes to avoid conflicts with XML/HTML
- Clear, unique delimiters that won't appear in normal text
- Any shell command allowed - LLM has full system access

**Output Format:** `<||SYSTEM.OUTPUT>result<SYSTEM.OUTPUT||>`
- Written to conversation file so LLM sees the result
- LLM continues generation after seeing output
- Prevents infinite loops where LLM doesn't know command was executed

---

## Current Implementation (TO BE COMMENTED OUT)

### Problem Statement
The current tool calling system relies on model-specific chat templates (ChatML, Mistral, Llama3, Gemma) to tell the LLM how to use tools. This is fragile because:
1. Different models have different tool calling formats
2. Template detection may fail or be incorrect
3. Tool calling instructions are embedded in complex template logic
4. Not all models understand their native tool format well

### Current Architecture (OLD - will be commented)

**1. Template System (`src/web/chat/templates.rs`)**
- Detects template type from GGUF metadata (`tokenizer.chat_template`)
- Injects tool definitions differently per template:
  - **ChatML (Qwen)**: Uses Hermes-style `<tools>...</tools>` and `<tool_call>...</tool_call>`
  - **Mistral**: Uses `[AVAILABLE_TOOLS]...[/AVAILABLE_TOOLS]` and `[TOOL_CALLS]...`
  - **Llama3**: No tool injection currently
  - **Gemma**: No tool injection currently

**2. Tool Definitions (`src/web/utils.rs`)**
- Provides JSON schema for 4 tools: `read_file`, `write_file`, `list_directory`, `bash`
- OS-aware (Windows vs Linux/macOS examples)

**3. Generation Loop (`src/web/chat/generation.rs`)**
- Has a `<COMMAND>...</COMMAND>` detection system (lines 329-384)
- When detected: extracts command, executes via `execute_command()`, injects output back
- This is **separate** from the model-specific tool formats

**4. Frontend Tool Parser (`src/utils/toolParser.ts`)**
- Parses multiple formats: Mistral, Llama3, Qwen
- Executes tools via `/api/tools/execute` endpoint
- Has agentic loop with MAX_TOOL_ITERATIONS = 5

---

## New Universal System

### Token Format

**Command execution:**
```
<||SYSTEM.EXEC>ls -la<SYSTEM.EXEC||>
```

**System output (injected by backend):**
```
<||SYSTEM.OUTPUT>
total 64
drwxr-xr-x  10 user  staff   320 Jan 15 10:00 .
drwxr-xr-x   5 user  staff   160 Jan 15 09:00 ..
-rw-r--r--   1 user  staff  1234 Jan 15 10:00 file.txt
<SYSTEM.OUTPUT||>
```

### Why This Format?
1. `||` pipes make it unique - won't conflict with XML `<tag>` or HTML
2. `SYSTEM.EXEC` is clear and descriptive
3. Symmetric closing tag `<SYSTEM.EXEC||>` is easy to detect
4. Same pattern for output `<||SYSTEM.OUTPUT>...<SYSTEM.OUTPUT||>`

### Execution Flow

```
1. LLM generates: "Let me check the files <||SYSTEM.EXEC>ls -la<SYSTEM.EXEC||>"

2. Backend detects closing tag, pauses generation

3. Backend executes command: ls -la

4. Backend writes to conversation file:
   ASSISTANT:
   Let me check the files <||SYSTEM.EXEC>ls -la<SYSTEM.EXEC||>
   <||SYSTEM.OUTPUT>
   total 64
   drwxr-xr-x  10 user  staff   320 Jan 15 10:00 .
   ...
   <SYSTEM.OUTPUT||>

5. Backend injects output tokens into context

6. LLM continues: "I can see there are 10 files..."

7. Final response includes both command and output
```

### CRITICAL: Preventing Infinite Loops

The output MUST be:
1. Written to conversation file immediately
2. Injected into the LLM context before resuming
3. Visible in the conversation history when reloaded

If the LLM doesn't see the output, it will:
- Think the command wasn't executed
- Try to execute it again
- Create an infinite loop

---

## System Prompt

```
You are a helpful AI assistant with full system access.

## Command Execution

You can execute ANY system command by wrapping it in special tags:

<||SYSTEM.EXEC>your_command_here<SYSTEM.EXEC||>

Examples:
- List files: <||SYSTEM.EXEC>ls -la<SYSTEM.EXEC||>
- Read file: <||SYSTEM.EXEC>cat /path/to/file.txt<SYSTEM.EXEC||>
- Create file: <||SYSTEM.EXEC>echo "content" > file.txt<SYSTEM.EXEC||>
- Run program: <||SYSTEM.EXEC>python script.py<SYSTEM.EXEC||>

After you write a command, the system will execute it and show the output in:
<||SYSTEM.OUTPUT>...output here...<SYSTEM.OUTPUT||>

Wait for the output before continuing your response.

## Current Environment
- OS: {os_name}
- Working Directory: {cwd}
- Shell: {shell}
```

---

## Implementation Plan

### Phase 1: Backend Detection (`generation.rs`)

```rust
const EXEC_OPEN: &str = "<||SYSTEM.EXEC>";
const EXEC_CLOSE: &str = "<SYSTEM.EXEC||>";
const OUTPUT_OPEN: &str = "<||SYSTEM.OUTPUT>\n";
const OUTPUT_CLOSE: &str = "\n<SYSTEM.OUTPUT||>";

// In token generation loop:
let mut in_exec_block = false;
let mut exec_start_pos = 0;

// Check if we just completed an exec block
if response.contains(EXEC_OPEN) && response.contains(EXEC_CLOSE) {
    // Find the command
    if let Some(start) = response.rfind(EXEC_OPEN) {
        if let Some(end) = response.rfind(EXEC_CLOSE) {
            if end > start {
                let command = &response[start + EXEC_OPEN.len()..end];

                // Execute command
                let output = execute_command(command);

                // Format output block
                let output_block = format!("{}{}{}", OUTPUT_OPEN, output, OUTPUT_CLOSE);

                // 1. Log to conversation file
                logger.log_token(&output_block);

                // 2. Inject into LLM context
                let output_tokens = model.str_to_token(&output_block, AddBos::Never)?;
                for token in output_tokens {
                    batch.clear();
                    batch.add(token, token_pos, &[0], true)?;
                    context.decode(&mut batch)?;
                    token_pos += 1;
                }

                // 3. Add to response string
                response.push_str(&output_block);

                // 4. Stream to frontend
                if let Some(ref sender) = token_sender {
                    sender.send(TokenData {
                        token: output_block.clone(),
                        tokens_used: token_pos,
                        max_tokens: context_size as i32,
                    });
                }
            }
        }
    }
}
```

### Phase 2: System Prompt (`templates.rs`)

```rust
fn get_universal_system_prompt() -> String {
    let os_name = std::env::consts::OS;
    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| ".".to_string());

    let shell = if os_name == "windows" { "cmd/powershell" } else { "bash" };

    format!(r#"You are a helpful AI assistant with full system access.

## Command Execution

You can execute ANY system command by wrapping it in special tags:

<||SYSTEM.EXEC>your_command_here<SYSTEM.EXEC||>

Examples:
- List files: <||SYSTEM.EXEC>dir<SYSTEM.EXEC||> (Windows) or <||SYSTEM.EXEC>ls -la<SYSTEM.EXEC||> (Linux/Mac)
- Read file: <||SYSTEM.EXEC>type file.txt<SYSTEM.EXEC||> (Windows) or <||SYSTEM.EXEC>cat file.txt<SYSTEM.EXEC||>
- Create file: <||SYSTEM.EXEC>echo content > file.txt<SYSTEM.EXEC||>

After you write a command, the system will execute it and show the output in:
<||SYSTEM.OUTPUT>...output here...<SYSTEM.OUTPUT||>

Wait for the output before continuing your response.

## Current Environment
- OS: {}
- Working Directory: {}
- Shell: {}
"#, os_name, cwd, shell)
}
```

### Phase 3: Frontend Display

The frontend should render:
- `<||SYSTEM.EXEC>...<SYSTEM.EXEC||>` as a **command block** (like a code block with "Execute" styling)
- `<||SYSTEM.OUTPUT>...<SYSTEM.OUTPUT||>` as an **output block** (terminal-style output)

```typescript
// In MessageBubble.tsx
const renderSystemBlocks = (content: string) => {
    // Parse SYSTEM.EXEC blocks
    const execRegex = /<\|\|SYSTEM\.EXEC>([\s\S]*?)<SYSTEM\.EXEC\|\|>/g;
    const outputRegex = /<\|\|SYSTEM\.OUTPUT>([\s\S]*?)<SYSTEM\.OUTPUT\|\|>/g;

    // Replace with styled components
    // ... render as special UI elements
};
```

---

## Files to Modify

| File | Changes |
|------|---------|
| `src/web/chat/templates.rs` | Comment old tool injection, add `get_universal_system_prompt()` |
| `src/web/chat/generation.rs` | Comment old `<COMMAND>` system, add `<||SYSTEM.EXEC>` detection |
| `src/web/utils.rs` | Comment out (keep for reference) |
| `src/components/organisms/MessageBubble.tsx` | Add rendering for SYSTEM.EXEC and SYSTEM.OUTPUT blocks |

---

## Conversation File Format

```
[2025-01-15 10:30:00] USER:
List the files in the current directory

[2025-01-15 10:30:01] ASSISTANT:
I'll list the files for you.

<||SYSTEM.EXEC>ls -la<SYSTEM.EXEC||>
<||SYSTEM.OUTPUT>
total 64
drwxr-xr-x  10 user  staff   320 Jan 15 10:00 .
drwxr-xr-x   5 user  staff   160 Jan 15 09:00 ..
-rw-r--r--   1 user  staff  1234 Jan 15 10:00 package.json
-rw-r--r--   1 user  staff  5678 Jan 15 10:00 README.md
<SYSTEM.OUTPUT||>

Here are the files in the current directory:
- package.json (1234 bytes)
- README.md (5678 bytes)
```

---

## Checklist

- [ ] Comment out old tool system in `templates.rs`
- [ ] Comment out old `<COMMAND>` detection in `generation.rs`
- [ ] Implement `<||SYSTEM.EXEC>` detection in generation loop
- [ ] Write output to conversation file
- [ ] Inject output tokens into LLM context
- [ ] Add `get_universal_system_prompt()` function
- [ ] Update frontend MessageBubble to render exec/output blocks
- [ ] Test with Qwen3-8B
- [ ] Test with Devstral
- [ ] Verify no infinite loops
