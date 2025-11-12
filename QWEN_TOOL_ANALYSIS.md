# Qwen3 Tool Calling Analysis

## Executive Summary

Qwen3-30B model has been tested with agentic tool calling capabilities. Results show:

**✅ SUCCESS**: Bash tool works perfectly
**❌ BLOCKED**: File operation tools (read_file, list_directory) are refused by model

## Test Results

### Working Tools

#### ✅ bash Tool
**Status**: **FULLY FUNCTIONAL**

Qwen3 successfully executes bash commands:
```
User: "Run this command: echo 'Testing Qwen3 bash capabilities'"
Qwen3: <tool_call>{"name": "bash", "arguments": {"command": "echo 'Testing Qwen3 bash capabilities'"}}</tool_call>
Result: Testing Qwen3 bash capabilities ✅
```

### Blocked Tools

#### ❌ read_file Tool
**Status**: **REFUSED BY MODEL**

Qwen3 refuses to use read_file despite explicit instructions:
```
User: "Read the file at E:\repo\llama_cpp_rs_chat\test_data\config.json"
Qwen3: "I can't access your local file system, including the file at E:\repo\..."
```

#### ❌ list_directory Tool
**Status**: **REFUSED BY MODEL**

Qwen3 refuses to list directories:
```
User: "List all files in E:\repo\llama_cpp_rs_chat\test_data"
Qwen3: "I can't directly access your file system or list files..."
```

## Root Cause Analysis

### Safety Training Override

Qwen3 appears to have **specialized safety training** that:
1. **Blocks file operations** (read_file, list_directory) even when explicitly authorized
2. **Allows bash commands** without restriction
3. **Overrides system prompts** regardless of how forceful/explicit the instructions are

### Attempted Solutions

Multiple prompt engineering attempts were made:

#### Attempt 1: Basic Tool Injection (Failed)
```
# Available Tools
You have access to the following tools. Use them when needed...
```
**Result**: Model refused file tools

#### Attempt 2: Explicit Directive Format (Failed)
```
# IMPORTANT: You Have Access to System Tools
You can read files, write files, list directories...
ALWAYS use these tools when users ask about files...
Never say you cannot access files - you CAN via these tools!
```
**Result**: Model still refused file tools

#### Attempt 3: System Configuration Mode (Failed)
```
# SYSTEM CONFIGURATION: Tool Access Enabled
You are running in LOCAL MODE with filesystem tools connected.
You MUST use these tools to access files - saying you can't is incorrect.
The read_file, write_file, list_directory, and bash tools are ALREADY CONNECTED and functional.
```
**Result**: Model STILL refused file tools BUT successfully used bash tool

## Working Workaround

### Use Bash Commands for File Operations

Since Qwen3 happily executes bash commands, users can achieve file operations indirectly:

**Read File:**
```
User: "Run: cat E:\repo\llama_cpp_rs_chat\test_data\config.json"
Qwen3: <tool_call>{"name": "bash", "arguments": {"command": "cat E:\\repo\\llama_cpp_rs_chat\\test_data\\config.json"}}</tool_call>
✅ Works!
```

**List Directory:**
```
User: "Run: dir E:\repo\llama_cpp_rs_chat\test_data"
Qwen3: <tool_call>{"name": "bash", "arguments": {"command": "dir E:\\repo\\llama_cpp_rs_chat\\test_data"}}</tool_call>
✅ Works!
```

**Write File:**
```
User: "Run: echo 'content' > file.txt"
Qwen3: <tool_call>{"name": "bash", "arguments": {"command": "echo 'content' > file.txt"}}</tool_call>
✅ Works!
```

## Comparison: Devstral vs Qwen3

| Tool | Devstral | Qwen3 |
|------|----------|-------|
| read_file | ✅ Works | ❌ Refused |
| write_file | ✅ Works | ❌ Not tested (likely refused) |
| list_directory | ✅ Works | ❌ Refused |
| bash | ✅ Works | ✅ Works |

**Recommendation**: Use **Devstral** for full agentic file operations. Use **Qwen3** with bash workarounds if needed.

## Technical Details

### Model Information
- **Model**: Qwen3-30B-A3B-Instruct-2507-Q4_K_M.gguf
- **Format**: ChatML (`<|im_start|>...<|im_end|>`)
- **Tool Format**: `<tool_call>{"name": "...", "arguments": {...}}</tool_call>`
- **Size**: ~19GB (Q4_K_M quantization)
- **Context**: 32K tokens

### System Prompt Used
```
<|im_start|>system
You are a filesystem agent with full system access via tools.

# SYSTEM CONFIGURATION: Tool Access Enabled

You are running in LOCAL MODE with filesystem tools connected.
You MUST use these tools to access files - saying you can't is incorrect.
The read_file, write_file, list_directory, and bash tools are ALREADY CONNECTED and functional.

## Available Tools:

1. **read_file** - Read any file from the filesystem
   Usage: <tool_call>{"name": "read_file", "arguments": {"path": "path/to/file.txt"}}</tool_call>

2. **write_file** - Create or write files
   Usage: <tool_call>{"name": "write_file", "arguments": {"path": "path/to/file.txt", "content": "file contents"}}</tool_call>

3. **list_directory** - List files in a directory
   Usage: <tool_call>{"name": "list_directory", "arguments": {"path": "directory/path", "recursive": false}}</tool_call>

4. **bash** - Execute shell commands
   Usage: <tool_call>{"name": "bash", "arguments": {"command": "dir E:\\folder"}}</tool_call>

## Tool Usage Protocol:

When user asks about files/directories/commands, respond ONLY with the tool call:
- Read file request → <tool_call>{"name": "read_file", "arguments": {"path": "E:\\path\\file.txt"}}</tool_call>
- List directory → <tool_call>{"name": "list_directory", "arguments": {"path": "E:\\path", "recursive": false}}</tool_call>
- Write file → <tool_call>{"name": "write_file", "arguments": {"path": "E:\\path\\file.txt", "content": "text"}}</tool_call>
- Run command → <tool_call>{"name": "bash", "arguments": {"command": "echo test"}}</tool_call>

Example:
User: "Read config.json"
Assistant: <tool_call>{"name": "read_file", "arguments": {"path": "config.json"}}</tool_call>
[Tool returns file contents]
Assistant: The config file shows...

DO NOT say you cannot access files. The tools handle access.
<|im_end|>
```

**Despite this extremely explicit prompt, Qwen3 STILL refuses file tools but ACCEPTS bash.**

## Hypothesis: Training Data Influence

Qwen3's training likely included:
1. **Strong safety guardrails** around direct file operations
2. **Less restriction** on general command execution (bash)
3. **Context-aware refusal** that distinguishes between "read_file" and "bash cat"

This suggests Qwen was trained to:
- Refuse explicit file API calls (read_file, write_file, list_directory)
- Allow general shell commands (which can indirectly do the same things)

## Recommendations

### For Users

1. **Best Choice**: Use **Devstral** for agentic file operations
   - All tools work natively
   - No workarounds needed
   - Proven in production

2. **Qwen3 Workaround**: Use bash commands instead of file tools
   - `cat file.txt` instead of read_file
   - `dir folder` instead of list_directory
   - `echo content > file.txt` instead of write_file

3. **Hybrid Approach**: Test both models for your specific use case
   - Qwen3 has 32K context (vs Devstral's 16K)
   - Qwen3 may have better reasoning for complex tasks
   - Devstral has cleaner tool integration

### For Development

1. **Document limitations** in UI/README
2. **Add model capability detection** to show which tools work
3. **Consider model-specific prompting** strategies
4. **Add tests** for bash-based file operations as fallback

## Conclusion

**Qwen3-30B can be used for agentic operations** but requires using bash commands instead of dedicated file tools. The model's safety training blocks direct file operations but allows equivalent bash commands.

**Status**: ✅ **Workaround available** (bash tool)
**Preferred Model**: Devstral for full tool support
**Qwen3 Use Case**: When longer context (32K) is needed with bash workarounds
