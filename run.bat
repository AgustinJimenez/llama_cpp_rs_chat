@echo off
REM Unified run script - all configuration via .env file
REM Usage: run.bat
set "CMAKE_DIR=C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin"
set "PATH=%CMAKE_DIR%;%PATH%"
echo 🚀 LLM Chat Runner

REM Set defaults
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

REM Handle different run modes
if /i "%RUN_MODE%"=="normal" (
    echo 🚀 Running in normal mode (clean output^)...
    cargo run 2>nul
) else if /i "%RUN_MODE%"=="debug_low" (
    echo 🐛 Running in debug_low mode (app debug messages^)...
    set LLAMA_LOG_LEVEL=4
    set LLAMA_DEBUG=true
    cargo run
) else if /i "%RUN_MODE%"=="debug_high" (
    echo 🔬 Running in debug_high mode (all logs including LLaMA.cpp^)...
    set LLAMA_LOG_LEVEL=0
    set LLAMA_DEBUG=true
    cargo run
) else if /i "%RUN_MODE%"=="build" (
    echo 🔨 Building LLaMA.cpp application...
    cargo build --release
) else (
    echo ⚠️  Unknown mode "%RUN_MODE%". Using normal mode...
    cargo run 2>nul
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