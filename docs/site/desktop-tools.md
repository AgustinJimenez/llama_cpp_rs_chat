# Desktop Tools

LLaMA Chat can control your desktop — click, type, take screenshots, read text from the screen. Enable "Tools" in the chat bar and ask the model to interact with your computer.

---

## Available tools

### Mouse and keyboard
| Tool | Description |
|------|-------------|
| `click_screen` | Click at x,y coordinates |
| `double_click_screen` | Double-click |
| `right_click_screen` | Right-click (opens context menus) |
| `move_mouse` | Move without clicking |
| `type_text` | Type text via keyboard simulation |
| `press_key` | Press a key or chord (e.g. `ctrl+c`, `alt+tab`) |
| `scroll_screen` | Scroll at position |
| `mouse_drag` | Click and drag |

### Screen reading
| Tool | Description |
|------|-------------|
| `take_screenshot` | Full screen capture → vision pipeline |
| `ocr_screen` | Extract all text from screen via OCR |
| `ocr_region` | OCR a specific region |
| `find_text_on_screen` | Locate text by content |

### Window management
| Tool | Description |
|------|-------------|
| `list_windows` | List open windows |
| `focus_window` | Bring a window to foreground |
| `get_window_text` | Get text content of a window |
| `get_ui_tree` | Accessibility UI tree of a window |

### Application scripting
| Tool | Description |
|------|-------------|
| `execute_app_script` | Run scripts inside apps (Blender bpy, Unity Editor, etc.) |

---

## Requirements

- **Windows**: all tools available via Win32 API + WinRT OCR
- **macOS**: screenshot + keyboard/mouse tools available; OCR via Vision framework
- **Linux**: screenshot + keyboard/mouse via X11/Wayland

For OCR on Linux, `tesseract-ocr` must be installed.

---

## Permissions

Desktop tools have access to **your entire screen and all open windows**. Only enable tools when you trust the model and the conversation context.

---

## Example: automate a UI task

```
User: Open Notepad, type "Hello world", save the file to the desktop.

Model: [takes screenshot] → [clicks Start menu] → [types "notepad"] →
       [clicks Notepad] → [types "Hello world"] → [presses Ctrl+S] →
       [types filename] → [presses Enter]
```

---

## GPU application scripting

For GPU-intensive apps (Blender, Unity, Godot, Maya, Unreal Engine), `execute_app_script` runs code inside the app's scripting engine:

```python
# Blender Python (bpy)
import bpy
bpy.ops.mesh.primitive_cube_add(size=2)
```

The model detects which app is active and uses the appropriate scripting API.
