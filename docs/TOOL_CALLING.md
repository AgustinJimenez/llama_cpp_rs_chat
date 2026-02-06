# Tool Calling Implementation

## Overview
This document explains the tool calling implementation in the LLaMA Chat application, which enables the model to execute commands and interact with the system.

## Architecture

### Backend (Rust)

#### Tool Definitions (`src/main_web.rs`)
The `get_available_tools_json()` function defines available tools in JSON schema format:

```rust
fn get_available_tools_json() -> String {
    serde_json::json!([
        {
            "type": "function",
            "function": {
                "name": "bash",
                "description": "Execute bash/shell commands on the system...",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "The shell command to execute"
                        }
                    },
                    "required": ["command"]
                }
            }
        }
    ]).to_string()
}
```

#### Chat Template Integration
The `apply_model_chat_template()` function injects tool definitions into the prompt:

1. Applies the model's chat template to conversation messages
2. Checks if the model supports tools (Mistral-style format)
3. Injects `[AVAILABLE_TOOLS]...[/AVAILABLE_TOOLS]` section after the system prompt
4. Returns the modified prompt with tool definitions

#### Tool Execution Endpoint (`/api/tools/execute`)
Handles execution of tool calls from the frontend:

```rust
POST /api/tools/execute
{
  "tool_name": "bash",
  "arguments": {
    "command": "ls -la"
  }
}
```

Returns:
```json
{
  "success": true,
  "result": "...",
  "exit_code": 0
}
```

### Frontend (TypeScript/React)

#### Tool Parser (`src/utils/toolParser.ts`)
Universal parser supporting multiple tool calling formats:

- **Mistral**: `[TOOL_CALLS]func_name[ARGS]{"arg": "value"}`
- **Llama3**: `<function=name>{"arg": "value"}</function>`
- **Qwen**: `<tool_call>{"name": "func", "arguments": {...}}</tool_call>`

Key functions:
- `autoParseToolCalls(text)`: Detects and parses tool calls from model output
- `stripToolCalls(text)`: Removes tool call markers for clean display

#### Agentic Loop (`src/hooks/useChat.ts`)
Automatic tool execution with iterative refinement:

1. Send user message to model
2. Model generates response with tool calls
3. Parse tool calls from response
4. Execute tools via `/api/tools/execute`
5. Format tool results as `[TOOL_RESULTS]...[/TOOL_RESULTS]`
6. Send results back to model
7. Model continues generation (repeat from step 2)
8. Safety limit: MAX_TOOL_ITERATIONS = 5

```typescript
const continueWithToolResults = async (toolResults: string) => {
  toolIterationCount.current += 1;
  if (toolIterationCount.current >= MAX_TOOL_ITERATIONS) {
    toast.error('Maximum tool iterations reached');
    return;
  }
  // Create new assistant message with tool results
  // Continue generation...
}
```

#### UI Display (`src/components/MessageBubble.tsx`)
Tool calls are displayed with special styling:

- Blue background container for tool calls
- JSON-formatted arguments in code block
- Tool call markers stripped from main content
- Separate display for tool call UI and response text

## Tool Calling Flow

```
User: "What files are in the current directory?"
  ↓
Model receives prompt with [AVAILABLE_TOOLS] section
  ↓
Model generates: [TOOL_CALLS]bash[ARGS]{"command": "ls -la"}
  ↓
Frontend parses tool call
  ↓
POST /api/tools/execute {"tool_name": "bash", "arguments": {...}}
  ↓
Backend executes command
  ↓
Returns result to frontend
  ↓
Frontend sends result back to model: [TOOL_RESULTS]<output>[/TOOL_RESULTS]
  ↓
Model generates final response with context from tool execution
```

## Supported Models

### Mistral/Devstral
- Uses `[TOOL_CALLS]` / `[AVAILABLE_TOOLS]` format
- Built-in system prompt includes tool usage instructions
- Template expects `tools` array in Jinja2 context

### Llama3
- Uses `<function=name>...</function>` format
- Requires system prompt with tool instructions

### Qwen
- Uses `<tool_call>...</tool_call>` format
- JSON-based tool definitions

## Testing

### E2E Tests (`tests/e2e/tool-calling.test.ts`)

Tests cover:
1. Tool call detection and UI display
2. Tool execution API (`/api/tools/execute`)
3. Tool parsing for different formats
4. Agentic loop with multiple iterations
5. MAX_TOOL_ITERATIONS safety limit
6. Tool call marker stripping
7. Error handling for failed commands
8. Tool arguments display in UI

Run tests:
```bash
npm run test:e2e-fast
```

## Configuration

### Adding New Tools

1. Update `get_available_tools_json()` in `src/main_web.rs`:
```rust
{
    "type": "function",
    "function": {
        "name": "new_tool",
        "description": "Description...",
        "parameters": {
            // JSON schema...
        }
    }
}
```

2. Add tool execution logic in `/api/tools/execute` endpoint:
```rust
match request.tool_name.as_str() {
    "bash" => { /* ... */ },
    "new_tool" => {
        // Implementation...
    },
    _ => { /* error */ }
}
```

3. No frontend changes needed - universal parser handles all formats!

## Troubleshooting

### Model Not Using Tools
- Check if `[AVAILABLE_TOOLS]` section appears in generated prompt
- Verify tool definitions are valid JSON schema
- Check model's chat template supports tool calling
- Review model's system prompt for tool usage instructions

### Tool Calls Not Parsed
- Check browser console for parsing errors
- Verify model's output format matches one of the supported formats
- Test parser with `autoParseToolCalls()` directly

### Infinite Loop / Max Iterations
- Model may be repeatedly calling tools without finishing response
- Check tool results format - should be clear and concise
- Review model's understanding of when to stop using tools
- Adjust MAX_TOOL_ITERATIONS if needed (default: 5)

## Future Improvements

- [ ] Support for more tool types (file operations, web search, etc.)
- [ ] Tool parameter validation against JSON schema
- [ ] Streaming tool execution updates
- [ ] Tool call history and replay
- [ ] User approval before executing sensitive commands
- [ ] Tool execution sandboxing/permissions system
- [ ] Multi-tool parallel execution
- [ ] Tool result caching
