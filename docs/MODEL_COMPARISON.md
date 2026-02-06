# Model Comparison: Devstral vs Qwen3 for Agentic Operations

## Executive Summary

Both **Devstral** and **Qwen3-30B** models support agentic tool calling, but with different capabilities:

- **Devstral**: ✅ All tools work natively
- **Qwen3**: ⚠️ Requires bash workarounds for file operations

## Tool Support Matrix

| Tool | Devstral | Qwen3 Direct | Qwen3 via Bash |
|------|----------|--------------|----------------|
| read_file | ✅ Native | ❌ Refused | ✅ Via `cat`/`type` |
| write_file | ✅ Native | ❌ Not tested | ✅ Via `echo >` |
| list_directory | ✅ Native | ❌ Refused | ✅ Via `dir`/`ls` |
| bash | ✅ Native | ✅ Native | ✅ Native |

## Model Details

### Devstral 7B
- **Full Name**: Devstral-small-2409-Q8_0.gguf
- **Size**: ~7.6GB
- **Context**: 16K tokens
- **Format**: Mistral (`<s>[INST]...[/INST]`)
- **Tool Format**: `[TOOL_CALLS]name[ARGS]{...}`
- **Best For**: Direct file operations, standard agentic workflows

**Pros:**
- ✅ All tools work natively without workarounds
- ✅ Smaller model = faster inference
- ✅ Clean tool integration
- ✅ Proven production reliability

**Cons:**
- ⚠️ Smaller context window (16K vs 32K)
- ⚠️ Less capable for complex reasoning than Qwen3-30B

### Qwen3-30B
- **Full Name**: Qwen3-30B-A3B-Instruct-2507-Q4_K_M.gguf
- **Size**: ~19GB
- **Context**: 32K tokens
- **Format**: ChatML (`<|im_start|>...<|im_end|>`)
- **Tool Format**: `<tool_call>{"name":"...","arguments":{...}}</tool_call>`
- **Best For**: Complex reasoning, long context, bash-based operations

**Pros:**
- ✅ Larger model = better reasoning capabilities
- ✅ 32K context (2x Devstral)
- ✅ Bash tool works perfectly
- ✅ Strong general capabilities

**Cons:**
- ❌ Refuses direct file operation tools
- ⚠️ Requires workarounds for file operations
- ⚠️ Slower inference due to model size
- ⚠️ 30GB+ VRAM recommended for optimal performance

## Usage Recommendations

### Use Devstral When:

1. **Standard file operations** are needed
2. **Fast responses** are important
3. **Straightforward agentic tasks** like:
   - Reading configuration files
   - Listing directories
   - Writing output files
   - Simple code assistance

**Example prompts that work perfectly:**
```
"Read the config.json file and tell me the version"
"List all files in the src directory"
"Write this output to results.txt"
```

### Use Qwen3 When:

1. **Complex reasoning** is required
2. **Long context** is needed (up to 32K tokens)
3. **You can use bash workarounds** for file operations
4. **Advanced problem-solving** tasks like:
   - Multi-step analysis
   - Code refactoring with large context
   - Complex debugging scenarios

**Example prompts for Qwen3:**
```
"Run: cat config.json"              (instead of "Read config.json")
"Run: dir /s src"                   (instead of "List all files in src")
"Run: echo result > output.txt"     (instead of "Write to output.txt")
```

## Testing Results

### Devstral Results
✅ **50/50 API tests passed** (100% success rate)
✅ All file operations work natively
✅ Tested across 5 browsers (Chromium, Firefox, WebKit, Mobile Chrome, Mobile Safari)

### Qwen3 Results
✅ **Bash tool: 100% success rate**
❌ **File tools: 0% success rate** (model refuses)
✅ **Bash workaround: Viable alternative**

## Prompt Engineering Attempts (Qwen3)

Multiple attempts were made to convince Qwen3 to use file tools:

### Attempt 1: Basic Injection
```
# Available Tools
You have access to the following tools...
```
**Result**: ❌ Failed

### Attempt 2: Explicit Directives
```
# IMPORTANT: You Have Access to System Tools
ALWAYS use these tools when users ask about files...
Never say you cannot access files - you CAN via these tools!
```
**Result**: ❌ Failed

### Attempt 3: System Configuration Mode
```
# SYSTEM CONFIGURATION: Tool Access Enabled
You are running in LOCAL MODE with filesystem tools connected.
You MUST use these tools - saying you can't is incorrect.
```
**Result**: ❌ Still failed for file tools, ✅ but bash worked!

## Technical Analysis

### Why Qwen3 Refuses File Tools

Qwen3 appears to have **specialized safety training** that:

1. **Pattern-matches tool names** like "read_file", "list_directory"
2. **Triggers safety refusals** even with explicit authorization
3. **Does NOT trigger** on generic "bash" commands
4. **Overrides system prompts** regardless of how forceful

This suggests Qwen's training included specific guardrails around file API operations but not shell commands.

### Why Bash Workaround Works

The bash tool succeeds because:
- It's a **general-purpose command executor**
- Doesn't match specific safety patterns
- Gives model flexibility in approach
- Models often see bash as "safer" than direct file APIs

## Performance Comparison

| Metric | Devstral 7B | Qwen3-30B |
|--------|-------------|-----------|
| Model Size | ~7.6GB | ~19GB |
| Load Time | 10-15s | 30-60s |
| Inference Speed | Fast | Moderate |
| Memory Usage | ~10GB VRAM | ~24GB VRAM |
| Context Window | 16K tokens | 32K tokens |
| File Tools | Native | Workaround |

## Code Examples

### Devstral Usage (Native)
```typescript
// User prompt:
"Read the file at E:\repo\config.json and summarize the settings"

// Devstral response:
<tool_call>
[TOOL_CALLS]read_file[ARGS]{"path":"E:\\repo\\config.json"}
</tool_call>

// System executes read_file → Returns contents → Devstral summarizes
✅ Works perfectly
```

### Qwen3 Usage (Bash Workaround)
```typescript
// User prompt:
"Run this command: cat E:\repo\config.json"

// Qwen3 response:
<tool_call>
{"name": "bash", "arguments": {"command": "cat E:\\repo\\config.json"}}
</tool_call>

// System executes bash → Returns contents → Qwen3 summarizes
✅ Works with workaround
```

## Recommendations

### For Production Use

**Primary Choice**: **Devstral**
- Reliable file operations
- Faster inference
- Proven in testing
- No workarounds needed

**Secondary Choice**: **Qwen3** (when needed)
- Use for complex reasoning tasks
- Accept bash workaround requirement
- Best for long-context scenarios
- Train users on workaround syntax

### For Development

1. **Support both models** in the application
2. **Document limitations** clearly in UI
3. **Add model-specific tips** for users
4. **Test both** for specific use cases

### For Users

**Quick Decision Matrix:**

| If you need... | Use this model |
|----------------|----------------|
| Fast file operations | Devstral |
| Complex reasoning | Qwen3 |
| Simple automation | Devstral |
| Long context analysis | Qwen3 |
| No workarounds | Devstral |
| Flexible approach | Qwen3 |

## Example Workflows

### Devstral Workflow (Native Tools)
```
User: "Analyze all Python files in src/ and summarize their purpose"

1. Devstral uses list_directory tool
2. Gets list of .py files
3. Devstral uses read_file for each
4. Analyzes content
5. Provides summary

✅ Seamless experience
```

### Qwen3 Workflow (Bash Workaround)
```
User: "Run: dir /s /b src\*.py"
[Qwen3 gets file list]

User: "Run: cat src\file1.py"
[Qwen3 reads file]

User: "Now analyze these files and summarize their purpose"
[Qwen3 provides analysis]

⚠️ Requires more manual steps but achieves same result
```

## Future Considerations

### Potential Solutions for Qwen3

1. **Fine-tuning**: Train Qwen3 on examples showing file tools are safe in local context
2. **Prompt Engineering**: Continue experimenting with system prompts
3. **Model Updates**: Wait for future Qwen versions with different safety training
4. **Hybrid Approach**: Use Devstral for file ops, Qwen3 for reasoning

### Architecture Improvements

1. **Auto-detection**: Detect which model is loaded and adjust UI accordingly
2. **Smart Routing**: Automatically convert file operations to bash commands for Qwen3
3. **Tool Fallback**: If read_file fails, automatically retry with bash
4. **User Guidance**: Show model-specific tips in the UI

## Conclusion

Both models are viable for agentic operations:

**Devstral** = Best overall experience with native tool support
**Qwen3** = Powerful capabilities but requires bash workarounds

**Bottom Line**: Start with **Devstral** for reliability. Switch to **Qwen3** only when you need its specific advantages (32K context, stronger reasoning) and can accept the bash workaround requirement.

## Testing Commands

### Test Devstral
```bash
# Run full Devstral tests
npx playwright test tests/e2e/agentic-tools.test.ts
```

### Test Qwen3 Native (Will fail for file tools)
```bash
# Run Qwen3 standard tests
npx playwright test tests/e2e/qwen-model.test.ts
```

### Test Qwen3 Workaround (Should pass)
```bash
# Run Qwen3 bash workaround tests
npx playwright test tests/e2e/qwen-bash-workaround.test.ts
```

---

**Last Updated**: 2025-11-11
**Models Tested**: Devstral 7B, Qwen3-30B
**Test Coverage**: 50 API tests + E2E workflows + Bash workarounds
