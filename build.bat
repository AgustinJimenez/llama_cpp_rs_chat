@echo off
REM Dedicated build script with CMake setup
REM This script ensures CMake is available for building llama-cpp-sys-2

echo 🔨 LLM Chat Builder

REM Set up CMake from Visual Studio Build Tools
set "CMAKE_DIR=C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin"
set "PATH=%CMAKE_DIR%;%PATH%"

REM Verify CMake is available
echo 🔍 Checking CMake availability...
cmake --version >nul 2>&1
if errorlevel 1 (
    echo ❌ CMake not found in PATH
    echo    Expected location: %CMAKE_DIR%
    echo    Please check Visual Studio Build Tools installation
    pause
    exit /b 1
) else (
    echo ✅ CMake found and available
)

REM Set defaults for build environment
if not defined LLAMA_STATIC_CRT set LLAMA_STATIC_CRT=ON
if not defined LLAMA_LOG_LEVEL set LLAMA_LOG_LEVEL=3

echo 🔧 Build Configuration:
echo    CMAKE_DIR=%CMAKE_DIR%
echo    LLAMA_STATIC_CRT=%LLAMA_STATIC_CRT%
echo    LLAMA_LOG_LEVEL=%LLAMA_LOG_LEVEL%

REM Parse command line arguments
set BUILD_MODE=
if "%1"=="--release" (
    set BUILD_MODE=--release
    echo 🚀 Building in RELEASE mode
) else if "%1"=="--debug" (
    set BUILD_MODE=
    echo 🐛 Building in DEBUG mode
) else if "%1"=="" (
    set BUILD_MODE=
    echo 🐛 Building in DEBUG mode (default)
) else (
    echo ❌ Unknown argument: %1
    echo Usage: build.bat [--release^|--debug]
    pause
    exit /b 1
)

echo.
echo 🔨 Starting Rust build...
echo    Command: cargo build %BUILD_MODE%

REM Execute the build
cargo build %BUILD_MODE%

if errorlevel 1 (
    echo.
    echo ❌ Build failed!
    echo    Check the error messages above for details
    pause
    exit /b 1
) else (
    echo.
    echo ✅ Build completed successfully!
    if "%BUILD_MODE%"=="--release" (
        echo    Binary location: target\release\main.exe
    ) else (
        echo    Binary location: target\debug\main.exe
    )
)

echo.
echo 💡 To run the application:
echo    run.bat          - Run with full configuration
echo    target\debug\main.exe     - Run directly (debug)
if "%BUILD_MODE%"=="--release" (
    echo    target\release\main.exe   - Run directly (release)
)

REM Optional pause
if "%PAUSE_ON_EXIT%"=="true" (
    pause
)