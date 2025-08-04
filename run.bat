@echo off
REM Unified run script - all configuration via .env file
REM Usage: run.bat
set "CMAKE_DIR=C:\Program Files\CMake\bin"
set "PATH=%CMAKE_DIR%;%PATH%"
set LLAMA_STATIC_CRT=ON
set RUSTFLAGS=-Ctarget-feature=+crt-static
echo 🚀 LLM Chat Runner

REM Set defaults
if not defined LOG_LEVEL set LOG_LEVEL=3
if not defined APP_DEBUG set APP_DEBUG=false
if not defined RUN_MODE set RUN_MODE=normal
if not defined PAUSE_ON_EXIT set PAUSE_ON_EXIT=true
if not defined BACKEND set BACKEND=llamacpp

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
echo    BACKEND=%BACKEND%
echo    LOG_LEVEL=%LOG_LEVEL%
echo    APP_DEBUG=%APP_DEBUG%

REM Set backend-specific environment variables
if /i "%BACKEND%"=="llamacpp" (
    set LLAMA_LOG_LEVEL=%LOG_LEVEL%
    set LLAMA_DEBUG=%APP_DEBUG%
    set BUILD_FEATURES=--no-default-features --features llamacpp
) else if /i "%BACKEND%"=="candle" (
    set BUILD_FEATURES=--no-default-features --features candle
) else (
    echo ❌ Invalid BACKEND value: %BACKEND%
    echo    Valid options: llamacpp, candle
    goto :error
)

REM Handle different run modes
if /i "%RUN_MODE%"=="silent" (
    echo 🔇 Running in silent mode (stderr suppressed^)...
    cargo run %BUILD_FEATURES% 2>nul
) else if /i "%RUN_MODE%"=="debug" (
    echo 🐛 Running in debug mode (all logs visible^)...
    set LOG_LEVEL=0
    set APP_DEBUG=true
    if /i "%BACKEND%"=="llamacpp" (
        set LLAMA_LOG_LEVEL=0
        set LLAMA_DEBUG=true
    )
    cargo run %BUILD_FEATURES%
) else if /i "%RUN_MODE%"=="build" (
    echo 🔨 Building with %BACKEND% backend...
    cargo build --release %BUILD_FEATURES%
) else (
    echo 🚀 Running in normal mode with %BACKEND% backend...
    cargo run %BUILD_FEATURES%
)

echo ✅ Execution completed

REM Pause if requested
if /i "%PAUSE_ON_EXIT%"=="true" (
    pause
)
goto :eof

:error
echo ❌ Build failed or invalid configuration
if /i "%PAUSE_ON_EXIT%"=="true" (
    pause
)
exit /b 1