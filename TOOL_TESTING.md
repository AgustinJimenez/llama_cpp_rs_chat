# Tool Testing Guide

This guide explains how to test the agentic tool calling capabilities.

## Quick Start

### 1. Start the Server

```bash
cargo run --bin llama_chat_web
```

Server should start on `http://localhost:8000`

### 2. Quick API Test (Windows)

```bash
test_tools.bat
```

This will test all tool endpoints directly without needing a browser or model.

### 3. Run Playwright API Tests

These test the tool endpoints programmatically:

```bash
# Install dependencies if needed
npm install

# Run tool API tests (no model required)
npx playwright test tests/e2e/tool-api.test.ts
```

**Expected Results:**
- ✅ 9 tests pass
- All tools work correctly (read_file, write_file, list_directory, bash)
- Error handling works

### 4. Run Agentic E2E Tests (Requires Model)

These tests verify the model can autonomously use tools:

```bash
# Load a model first via the web UI at http://localhost:8000
# Then run:
npx playwright test tests/e2e/agentic-tools.test.ts
```

**Expected Results:**
- Model reads files when asked
- Model lists directories when requested
- Model executes bash commands
- Model handles multi-step workflows

## Test Files Created

### Test Data Files (`test_data/`)
- `sample_file.txt` - Plain text file for reading tests
- `config.json` - JSON file for structured data tests
- `README.md` - Markdown test file
- `test_output.txt` - Created by write_file tests

### Test Suites

#### `tests/e2e/tool-api.test.ts` (9 tests)
Direct API endpoint testing without model:
1. ✅ Read plain text file
2. ✅ Read JSON file
3. ✅ Handle non-existent file errors
4. ✅ Create new file (write_file)
5. ✅ List directory (non-recursive)
6. ✅ List directory (recursive)
7. ✅ Execute bash echo command
8. ✅ Execute bash directory listing
9. ✅ Handle unknown tool errors

#### `tests/e2e/agentic-tools.test.ts` (4 tests)
End-to-end agentic behavior testing (requires loaded model):
1. 🤖 Model autonomously uses read_file
2. 🤖 Model autonomously uses list_directory
3. 🤖 Model autonomously uses bash
4. 🤖 Model executes multi-step workflow

## Manual Testing in Browser

1. **Start server**: `cargo run --bin llama_chat_web`
2. **Open browser**: http://localhost:8000
3. **Load model**: Click "Select a model" and load Devstral or similar
4. **Test prompts**:

```
"Read the file at E:\repo\llama_cpp_rs_chat\test_data\sample_file.txt"
```

```
"List all files in E:\repo\llama_cpp_rs_chat\test_data"
```

```
"What is the version in E:\repo\llama_cpp_rs_chat\test_data\config.json?"
```

```
"Create a file called greeting.txt with the content 'Hello World'"
```

```
"Show me all .rs files in the src directory"
```

## Expected Tool Call Format

When the model generates tool calls, they appear as:

```
[TOOL_CALLS]read_file[ARGS]{"path":"E:\\repo\\test.txt"}
```

```
[TOOL_CALLS]write_file[ARGS]{"path":"output.txt","content":"Hello"}
```

```
[TOOL_CALLS]list_directory[ARGS]{"path":"E:\\repo","recursive":true}
```

```
[TOOL_CALLS]bash[ARGS]{"command":"dir E:\\repo"}
```

## Verification Checklist

- [ ] Server starts successfully on port 8000
- [ ] Test data files exist in `test_data/` directory
- [ ] Quick batch test (`test_tools.bat`) completes successfully
- [ ] Playwright API tests (9/9 pass)
- [ ] Model loads in browser UI
- [ ] Model receives `[AVAILABLE_TOOLS]` in prompt (check browser console)
- [ ] Model generates `[TOOL_CALLS]` when asked to use tools
- [ ] Frontend auto-executes tool calls
- [ ] Tool results appear in response
- [ ] Agentic E2E tests pass (if model loaded)

## Troubleshooting

### Tests Fail: "Connection refused"
- Ensure server is running: `cargo run --bin llama_chat_web`
- Check port 8000 is not in use by another process

### Tool Calls Not Working
- Verify model is Mistral-style (Devstral, Mistral, etc.)
- Check browser console for `[AVAILABLE_TOOLS]` in prompts
- Ensure frontend has latest tool parser (`src/utils/toolParser.ts`)

### Read/Write File Errors
- Check file paths are absolute or relative to server working directory
- Verify file permissions on Windows/Linux
- Test manually with `test_tools.bat` first

### Model Not Using Tools
- Try more explicit prompts: "Use the read_file tool to read..."
- Check model temperature (try 0.7)
- Verify model supports tool calling format

## Architecture

```
User Request
    ↓
Model sees [AVAILABLE_TOOLS] in prompt
    ↓
Model generates [TOOL_CALLS]bash[ARGS]{...}
    ↓
Frontend parses tool call (toolParser.ts)
    ↓
POST /api/tools/execute
    ↓
Backend executes tool (main_web.rs)
    ↓
Returns result to frontend
    ↓
Frontend sends [TOOL_RESULTS] back to model
    ↓
Model generates final response with context
```

## Available Tools

1. **read_file** - Read any file from filesystem
2. **write_file** - Create or write files
3. **list_directory** - List directory contents (recursive option)
4. **bash** - Execute any shell command

All tools have full system access - use with trusted models only!
