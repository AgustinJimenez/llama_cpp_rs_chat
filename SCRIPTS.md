# 🚀 Unified Run Scripts

Simple, single-script solution with all configuration managed via `.env` file.

## 📁 Available Scripts (Just Two!)

### 🖥️ **Unix/Linux/macOS**
```bash
./run.sh
```

### 🪟 **Windows**  
```bat
run.bat
```

That's it! All behavior is controlled by your `.env` file.

## 🔧 Configuration

### **Step 1: Create your config**
```bash
cp .env.example .env
```

### **Step 2: Edit .env for your needs**
```env
# Change this line to control behavior:
RUN_MODE=silent        # Options: normal, silent, debug

# Fine-tune logging:
LLAMA_LOG_LEVEL=3      # 0=debug, 1=info, 2=warn, 3=error, 4=none  
LLAMA_DEBUG=false      # true/false for app debug output

# Script behavior:
PAUSE_ON_EXIT=false    # true/false (Windows only)
```

### **Step 3: Run**
```bash
./run.sh    # Unix/Linux/macOS
run.bat     # Windows
```

## 🎯 Run Mode Options

| Mode | Description | When to Use |
|------|-------------|-------------|
| `normal` | Standard execution with configured log levels | Development, general use |
| `silent` | Suppresses stderr (cleanest output) | Production, clean demos |
| `debug` | Shows ALL logs (overrides LLAMA_LOG_LEVEL to 0) | Troubleshooting issues |

## 📋 Quick Setup Examples

### **For Clean Output (Recommended)**
```env
RUN_MODE=silent
LLAMA_LOG_LEVEL=3
LLAMA_DEBUG=false
```

### **For Development**
```env
RUN_MODE=normal  
LLAMA_LOG_LEVEL=3
LLAMA_DEBUG=false
```

### **For Troubleshooting**
```env
RUN_MODE=debug
# LLAMA_LOG_LEVEL automatically set to 0
# LLAMA_DEBUG automatically set to true
```

## 🔧 Script Features

✅ **Auto .env loading** - Reads your configuration automatically  
✅ **Sensible defaults** - Works even without .env file  
✅ **Cross-platform** - Same behavior on Windows, macOS, Linux  
✅ **Visual feedback** - Shows current settings before running  
✅ **One script per platform** - No more confusion!

## 🛠️ Permissions (Unix/Linux/macOS)

Make script executable:
```bash
chmod +x run.sh
```

## 💡 Pro Tips

- **No .env file?** Scripts use sensible defaults
- **Windows double-click?** Set `PAUSE_ON_EXIT=true` 
- **Multiple configs?** Use `.env.development`, `.env.production`, etc. and copy as needed
- **Override on-the-fly?** `RUN_MODE=debug ./run.sh`