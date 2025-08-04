@echo off
REM Unified run script - all configuration via .env file
REM Usage: run.bat

echo 🚀 LLaMA.cpp Chat Runner

REM Set defaults
if not defined LLAMA_LOG_LEVEL set LLAMA_LOG_LEVEL=3
if not defined LLAMA_DEBUG set LLAMA_DEBUG=false
if not defined RUN_MODE set RUN_MODE=normal
if not defined PAUSE_ON_EXIT set PAUSE_ON_EXIT=true

REM Load .env file if it exists
if exist .env (
    echo 📄 Loading configuration from .env...
    for /f "eol=# tokens=1,2 delims==" %%a in (.env) do (
        if not "%%b"=="" set %%a=%%b
    )
) else (
    echo ⚙️  No .env file found - using defaults
    echo    💡 Copy .env.example to .env to customize settings
)

REM Display current configuration
echo 🔧 Configuration:
echo    RUN_MODE=%RUN_MODE%
echo    LLAMA_LOG_LEVEL=%LLAMA_LOG_LEVEL%
echo    LLAMA_DEBUG=%LLAMA_DEBUG%

REM Handle different run modes
if /i "%RUN_MODE%"=="silent" (
    echo 🔇 Running in silent mode (stderr suppressed^)...
    cargo run 2>nul
) else if /i "%RUN_MODE%"=="debug" (
    echo 🐛 Running in debug mode (all logs visible^)...
    set LLAMA_LOG_LEVEL=0
    set LLAMA_DEBUG=true
    cargo run
) else (
    echo 🚀 Running in normal mode...
    cargo run
)

echo ✅ Execution completed

REM Pause if requested
if /i "%PAUSE_ON_EXIT%"=="true" (
    pause
)