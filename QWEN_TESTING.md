# Testing Qwen3 Model with Tool Calling

This guide explains how to test the Qwen3-30B model with agentic tool calling.

## Model Information

**Model:** Qwen3-30B-A3B-Instruct-2507-Q4_K_M.gguf
**Path:** `E:\.lmstudio\models\lmstudio-community\Qwen3-30B-A3B-Instruct-2507-GGUF\Qwen3-30B-A3B-Instruct-2507-Q4_K_M.gguf`
**Format:** ChatML (`<|im_start|>...<|im_end|>`)
**Tool Format:** `<tool_call>{"name": "tool_name", "arguments": {...}}</tool_call>`

## Changes Made for Qwen Support

### Backend (`src/web/chat_handler.rs`)

Added tool injection for ChatML/Qwen models:

```rust
Some("ChatML") => {
    // Inject tool definitions for Qwen models
    let tools_json = get_available_tools_json();
    system_content.push_str("\n\n# Available Tools\n");
    system_content.push_str("You have access to the following tools...\n");
    system_content.push_str(&tools_json);

    p.push_str("<|im_start|>system\n");
    p.push_str(&system_content);
    p.push_str("<|im_end|>\n");
    // ... rest of template
}
```

### Frontend (`src/utils/toolParser.ts`)

Already supports Qwen format:
```typescript
// Qwen: <tool_call>{"name": "func", "arguments": {...}}</tool_call>
const qwenParser: ToolParser = {
  detect(text: string): boolean {
    return text.includes('<tool_call>');
  },
  parse(text: string): ToolCall[] {
    // Parses Qwen tool calls
  }
};
```

## Testing Procedure

### 1. Rebuild the Server (after killing old process)

```bash
# Find and kill process on port 8000
netstat -ano | findstr :8000
# Note the PID from the last column

# Kill it (replace XXXXX with actual PID)
taskkill /PID XXXXX /F

# Rebuild
cargo build --bin llama_chat_web

# Start server
cargo run --bin llama_chat_web
```

### 2. Run Automated Tests

```bash
# Run Qwen-specific tests
npx playwright test tests/e2e/qwen-model.test.ts
```

**Tests Included:**
1. Load Qwen3 model via UI
2. Test file reading with tool calling
3. Test directory listing
4. Test bash command execution

### 3. Manual Testing in Browser

1. Open http://localhost:8000
2. Click "Select a model to load"
3. Enter model path:
   ```
   E:\.lmstudio\models\lmstudio-community\Qwen3-30B-A3B-Instruct-2507-GGUF\Qwen3-30B-A3B-Instruct-2507-Q4_K_M.gguf
   ```
4. Set GPU layers to maximum
5. Click "Load Model"
6. Wait for model to load (may take 30-60 seconds for 30B model)

### 4. Test Prompts for Qwen3

Try these prompts to verify tool calling:

#### Read File Test
```
Read the file at E:\repo\llama_cpp_rs_chat\test_data\config.json and tell me the version
```

**Expected:** Model uses `read_file` tool and reports version "1.0"

#### List Directory Test
```
Show me all files in E:\repo\llama_cpp_rs_chat\test_data
```

**Expected:** Model uses `list_directory` tool and lists:
- sample_file.txt
- config.json
- README.md
- test_output.txt

#### Bash Command Test
```
Run the command: echo "Hello from Qwen3"
```

**Expected:** Model uses `bash` tool and returns command output

#### Write File Test
```
Create a file called qwen_test.txt with the content "Tested by Qwen3-30B model"
```

**Expected:** Model uses `write_file` tool to create the file

## Tool Format Comparison

### Mistral/Devstral Format
```
[TOOL_CALLS]read_file[ARGS]{"path":"E:\\repo\\file.txt"}
```

### Qwen Format
```
<tool_call>{"name": "read_file", "arguments": {"path": "E:\\repo\\file.txt"}}</tool_call>
```

### Llama3 Format
```
<function=read_file>{"path": "E:\\repo\\file.txt"}</function>
```

All three formats are supported by the frontend tool parser!

## Verifying Tool Injection

To verify tools are being sent to Qwen3:

1. Open browser console (F12)
2. Send a message to the model
3. Check the WebSocket messages for the prompt
4. You should see in the system message:

```
<|im_start|>system
You are a helpful AI assistant.

# Available Tools
You have access to the following tools. Use them when needed by generating tool calls in this format:
<tool_call>{"name": "tool_name", "arguments": {"arg1": "value1"}}</tool_call>

Available tools:
[{"type":"function","function":{"name":"read_file",...}}]
<|im_end|>
```

## Troubleshooting

### Model Not Using Tools
- Check browser console for system prompt - verify tools are listed
- Try more explicit prompts: "Use the read_file tool to read..."
- Qwen3-30B should be very capable with tool calling

### Tool Calls Not Detected
- Verify `src/utils/toolParser.ts` has qwenParser
- Check that Qwen format `<tool_call>` is being parsed

### Build Errors
- Make sure old server process is fully stopped
- Check that port 8000 is not in use
- Try: `cargo clean && cargo build`

## Performance Notes

**Qwen3-30B Model:**
- Model size: ~19GB (Q4_K_M quantization)
- Loading time: 30-60 seconds
- Inference speed: Depends on GPU (slower than 7B models)
- Context: 32K tokens
- Very capable with complex reasoning and tool use

## Next Steps

After successful testing:
1. Compare Qwen3 vs Devstral tool calling accuracy
2. Test complex multi-step workflows
3. Benchmark response quality for different tasks
4. Document best practices for each model type

## Architecture

```
User: "Read file X"
    ↓
ChatML Template Applied
    ↓
<|im_start|>system
Available Tools: [read_file, write_file, list_directory, bash]
<|im_end|>
    ↓
Qwen3 Model Processes
    ↓
Generates: <tool_call>{"name":"read_file","arguments":{...}}</tool_call>
    ↓
Frontend Parses (qwenParser)
    ↓
POST /api/tools/execute
    ↓
Backend Executes Tool
    ↓
Results Sent Back
    ↓
Qwen3 Generates Final Response
```

## Success Criteria

✅ Qwen3 model loads successfully
✅ Tools appear in system prompt
✅ Model generates `<tool_call>` format
✅ Frontend parses and executes tools
✅ Model incorporates results in response
✅ All 4 tools work correctly

Test both models (Devstral + Qwen3) to ensure universal tool calling support!
