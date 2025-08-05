# AI Capabilities Test

## Problem Fixed
The AI assistant was not aware of its file system and command execution capabilities. When users asked about files like TODO.md, it would say "I don't see any files" instead of using its tools to check.

## Solution Implemented
1. **Enhanced System Prompt** - Updated the system prompt to clearly instruct the AI about its capabilities
2. **Added Directory Listing** - Added `/list-dir` and `/ls` commands for exploring directories
3. **Clear Instructions** - Told the AI to ALWAYS check for files before saying they don't exist

## Updated System Prompt
The AI now receives this clear instruction:
```
You are an advanced AI assistant with powerful capabilities. You can:

IMPORTANT: When users ask about files, always check if they exist first using /read-file <path>.

File Operations:
- /list-dir [path] - List directory contents (use /ls for short)
- /read-file <path> - Read any file (use this to check file contents)
- /create-file <path> <content> - Create new files
- /modify-file <path> <line> <content> - Edit files
- /delete-file <path> - Delete files
- /create-dir <path> - Create directories

System Commands:
- /execute <command> - Run system commands safely
- /list-templates - Show available project templates
- /create-project <template> <name> - Generate complete projects

When users ask about files (like TODO.md), ALWAYS:
1. First use /list-dir to see what files are available
2. Then use /read-file <filename> to read specific files
Don't say files don't exist without checking first! Always explore the directory structure.

Use /help to see all available commands.
```

## Test Cases
Now when users ask:
1. **"What's in the TODO.md file?"** - AI should use `/list-dir` then `/read-file TODO.md`
2. **"What files are in this directory?"** - AI should use `/list-dir`
3. **"Create a new project"** - AI should use `/list-templates` then `/create-project`
4. **"Show me the project structure"** - AI should use `/list-dir` and explore subdirectories

## Commands Available
- `/list-dir` or `/ls` - List directory contents
- `/read-file <path>` - Read any file
- `/create-file <path> <content>` - Create files
- `/execute <command>` - Run system commands
- `/create-project <template> <name>` - Generate projects
- `/help` - Show all commands

The AI should now proactively use these tools instead of claiming it can't see files or directories.