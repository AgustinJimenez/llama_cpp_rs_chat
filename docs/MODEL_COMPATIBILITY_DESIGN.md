# Multi-Model Compatibility Design

## Goal
Support multiple LLM models with **consistent tool-calling behavior**, regardless of each model's specific limitations or training biases.

## Problem Statement

Different models have different capabilities and limitations:

| Model | read_file | write_file | list_directory | bash |
|-------|-----------|------------|----------------|------|
| Devstral | ✅ Native | ✅ Native | ✅ Native | ✅ Native |
| Qwen3 | ❌ Refused | ❌ Refused | ❌ Refused | ✅ Native |
| Llama3 | ❓ Unknown | ❓ Unknown | ❓ Unknown | ❓ Unknown |

**User Expectation**: "Read this file" should work the same regardless of which model is loaded.

## Architecture

### Current Flow (Broken for Some Models)
```
User: "Read config.json"
  ↓
Model generates: read_file tool call
  ↓
(Qwen3 refuses here)
  ↓
❌ Failure
```

### Proposed Flow (Model-Agnostic)
```
User: "Read config.json"
  ↓
Model generates: read_file tool call
  ↓
Backend checks model capabilities
  ↓
├─ If model supports read_file natively: Execute directly
└─ If model refuses (Qwen3): Convert to bash equivalent
  ↓
Execute: bash("cat config.json")
  ↓
Return results in unified format
  ↓
✅ Success
```

## Implementation Strategy

### Option 1: Backend Tool Translation (Recommended)

**Location**: `src/main_web.rs` - `/api/tools/execute` endpoint

```rust
// Detect which model is loaded
let model_type = get_current_model_type();

// Check if tool is supported by this model
let tool_request = match (&model_type, tool_name.as_str()) {
    // Qwen3 doesn't support file tools - translate to bash
    ("ChatML" | "Qwen", "read_file") => {
        let path = get_arg("path");
        ToolRequest {
            tool_name: "bash",
            arguments: json!({
                "command": if cfg!(windows) {
                    format!("type \"{}\"", path)
                } else {
                    format!("cat \"{}\"", path)
                }
            })
        }
    }

    ("ChatML" | "Qwen", "list_directory") => {
        let path = get_arg("path");
        let recursive = get_arg("recursive").unwrap_or(false);
        ToolRequest {
            tool_name: "bash",
            arguments: json!({
                "command": if cfg!(windows) {
                    format!("dir {}{}", if recursive { "/s " } else { "" }, path)
                } else {
                    format!("ls {}{}", if recursive { "-R " } else { "" }, path)
                }
            })
        }
    }

    // All other cases: use tool directly
    _ => original_request
};

// Execute the (possibly translated) tool
execute_tool(tool_request)
```

**Pros**:
- ✅ Transparent to user and frontend
- ✅ Model-specific quirks handled in one place
- ✅ Easy to add new model support
- ✅ No frontend changes needed

**Cons**:
- ⚠️ Backend needs to know which model is loaded
- ⚠️ Adds complexity to tool execution

### Option 2: Frontend Prompt Engineering

**Location**: `src/hooks/useChat.ts`

Modify prompts based on detected model:
```typescript
if (modelType === "qwen") {
  // For Qwen, user must explicitly ask for bash commands
  userPrompt = convertToBasPrompt(originalPrompt);
}
```

**Pros**:
- ✅ No backend changes

**Cons**:
- ❌ User experience differs per model
- ❌ Users must know about workarounds
- ❌ Breaks "consistent behavior" goal

### Option 3: Dual Tool Injection

**Location**: `src/web/chat_handler.rs`

For models like Qwen, inject BOTH file tools AND bash tools, with explicit fallback instructions:

```rust
system_content.push_str("
IMPORTANT: If you cannot use read_file, use bash with:
- Windows: type \"path\"
- Linux: cat \"path\"

If you cannot use list_directory, use bash with:
- Windows: dir \"path\"
- Linux: ls \"path\"
");
```

**Pros**:
- ✅ Gives model autonomy to choose
- ✅ No backend translation needed

**Cons**:
- ❌ Already tried this - models with safety training still refuse
- ❌ Unreliable

## Recommended Solution: **Option 1 (Backend Translation)**

### Implementation Plan

#### Phase 1: Model Type Detection

Add to `src/web/models.rs`:
```rust
pub fn get_model_capabilities(chat_template: &str) -> ModelCapabilities {
    match chat_template {
        "ChatML" => ModelCapabilities {
            native_file_tools: false,  // Qwen refuses these
            bash_tool: true,
            requires_translation: true,
        },
        "Mistral" | "Devstral" => ModelCapabilities {
            native_file_tools: true,
            bash_tool: true,
            requires_translation: false,
        },
        _ => ModelCapabilities::default(),
    }
}
```

#### Phase 2: Tool Translation Layer

Add to `src/main_web.rs`:
```rust
fn translate_tool_for_model(
    tool_name: &str,
    arguments: &serde_json::Value,
    model_capabilities: &ModelCapabilities
) -> (String, serde_json::Value) {

    if !model_capabilities.native_file_tools {
        // Model doesn't support file tools natively - convert to bash
        match tool_name {
            "read_file" => {
                let path = arguments["path"].as_str().unwrap();
                let cmd = if cfg!(windows) {
                    format!("type \"{}\"", path)
                } else {
                    format!("cat \"{}\"", path)
                };
                ("bash".to_string(), json!({"command": cmd}))
            }
            "write_file" => {
                let path = arguments["path"].as_str().unwrap();
                let content = arguments["content"].as_str().unwrap();
                let cmd = if cfg!(windows) {
                    format!("echo {} > \"{}\"", content, path)
                } else {
                    format!("echo '{}' > \"{}\"", content, path)
                };
                ("bash".to_string(), json!({"command": cmd}))
            }
            "list_directory" => {
                let path = arguments["path"].as_str().unwrap();
                let recursive = arguments.get("recursive")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let cmd = if cfg!(windows) {
                    format!("dir {}{}", if recursive { "/s " } else { "" }, path)
                } else {
                    format!("ls {}{}", if recursive { "-R " } else { "" }, path)
                };
                ("bash".to_string(), json!({"command": cmd}))
            }
            _ => (tool_name.to_string(), arguments.clone())
        }
    } else {
        // Model supports tools natively
        (tool_name.to_string(), arguments.clone())
    }
}
```

#### Phase 3: Integration

Modify `/api/tools/execute` endpoint:
```rust
(&Method::POST, "/api/tools/execute") => {
    // Get current model's capabilities
    let capabilities = {
        let state = shared_state.lock().await;
        get_model_capabilities(
            state.chat_template_type.as_deref().unwrap_or("Unknown")
        )
    };

    // Translate tool if needed
    let (tool_name, arguments) = translate_tool_for_model(
        &request.tool_name,
        &request.arguments,
        &capabilities
    );

    // Execute the (possibly translated) tool
    let result = execute_tool(&tool_name, &arguments);
    // ...
}
```

## Benefits of This Approach

### For Users
- ✅ **Consistent experience**: "Read file" works regardless of model
- ✅ **No workarounds needed**: Don't need to know about bash commands
- ✅ **Transparent**: Tool translation happens automatically

### For Developers
- ✅ **Centralized compatibility logic**: One place to handle model quirks
- ✅ **Easy to extend**: Add new models by updating capability map
- ✅ **Testable**: Can mock model capabilities in tests

### For Model Support
- ✅ **Gradual rollout**: Can add partial support for new models
- ✅ **Fallback chains**: Try native → bash → report error
- ✅ **Feature flags**: Enable/disable features per model

## Testing Strategy

### Unit Tests
```rust
#[test]
fn test_qwen_read_file_translation() {
    let caps = ModelCapabilities { native_file_tools: false, ..Default::default() };
    let (tool, args) = translate_tool_for_model(
        "read_file",
        &json!({"path": "config.json"}),
        &caps
    );
    assert_eq!(tool, "bash");
    assert!(args["command"].as_str().unwrap().contains("config.json"));
}
```

### Integration Tests
```typescript
test('qwen - translated file reading', async ({ page }) => {
  // Load Qwen3
  await loadModel('qwen3');

  // Ask to read file (using native tool call format)
  await sendMessage('Read config.json');

  // Should work transparently via bash translation
  expect(response).toContain('version');
});
```

## Future Enhancements

### 1. Capability Discovery
Auto-detect model capabilities by testing tool calls on first load:
```rust
async fn discover_model_capabilities(model: &Model) -> ModelCapabilities {
    let mut caps = ModelCapabilities::default();

    // Test read_file
    if test_tool_call(model, "read_file", test_file).await {
        caps.native_file_tools = true;
    }

    // Test bash
    if test_tool_call(model, "bash", "echo test").await {
        caps.bash_tool = true;
    }

    caps
}
```

### 2. Performance Optimization
Cache bash command outputs for repeated file reads:
```rust
let cache_key = format!("{}:{}", tool_name, path);
if let Some(cached) = tool_cache.get(&cache_key) {
    return cached;
}
```

### 3. Hybrid Approach
Try native first, fall back to bash automatically:
```rust
match execute_tool_native(tool_name, args) {
    Ok(result) => result,
    Err(_) if capabilities.bash_tool => {
        // Native failed, try bash fallback
        let (bash_tool, bash_args) = translate_to_bash(tool_name, args);
        execute_tool_native(bash_tool, bash_args)?
    }
    Err(e) => return Err(e),
}
```

## Conclusion

With backend tool translation, we can:
- ✅ Support multiple models with different capabilities
- ✅ Provide consistent user experience across all models
- ✅ Handle model-specific quirks transparently
- ✅ Easily add support for new models

**Next Step**: Implement Option 1 (Backend Translation) in `src/main_web.rs`
