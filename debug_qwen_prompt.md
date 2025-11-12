# Debug: Qwen Tool Calling Issue

## Problem
Qwen3 model loads successfully but doesn't use tools. It responds with:
> "I can't access your local file system..."

## Investigation Needed

### Check 1: Is the system prompt being sent?
- Need to verify tools are in the system message
- Check browser console / network tab for actual prompt sent to model

### Check 2: Qwen tool format
According to Qwen documentation, the expected format might be different.

Qwen typically expects tools in OpenAI format in the system prompt, like:
```
You are a helpful assistant with access to the following functions:

Function: read_file
Description: Read the complete contents of any file...
Parameters:
  - path (string): Absolute or relative path to file

To use a function, respond with:
<tool_call>
{"name": "function_name", "arguments": {"arg1": "value"}}
</tool_call>
```

### Current Implementation
We're adding to system prompt:
```
# Available Tools
You have access to the following tools. Use them when needed by generating tool calls in this format:
<tool_call>{"name": "tool_name", "arguments": {"arg1": "value1"}}</tool_call>

Available tools:
[{"type":"function","function":{"name":"read_file",...}}]
```

## Possible Issues

1. **JSON format might be confusing** - Qwen might not parse OpenAI-style JSON schema
2. **Instructions might not be clear enough** - Need more explicit examples
3. **Tool definitions might need to be formatted differently** - Try human-readable format

## Solution: Try More Explicit Format

Instead of JSON schema, use plain text descriptions:
```
You have access to these tools. Always use them when the user asks for file operations:

TOOL: read_file
- Use this to read any file from the filesystem
- Format: <tool_call>{"name": "read_file", "arguments": {"path": "full/path/to/file.txt"}}</tool_call>
- Example: User asks "read config.json" -> <tool_call>{"name": "read_file", "arguments": {"path": "config.json"}}</tool_call>

TOOL: write_file
- Use this to create or write files
- Format: <tool_call>{"name": "write_file", "arguments": {"path": "path/to/file.txt", "content": "file contents"}}</tool_call>

TOOL: list_directory
- Use this to list files in a directory
- Format: <tool_call>{"name": "list_directory", "arguments": {"path": "directory/path", "recursive": false}}</tool_call>

TOOL: bash
- Use this to execute shell commands
- Format: <tool_call>{"name": "bash", "arguments": {"command": "dir E:\\folder"}}</tool_call>

IMPORTANT: When users ask about files, directories, or commands, you MUST use these tools. Never say you can't access the filesystem - you can via these tools!
```

This makes it crystal clear what to do.
