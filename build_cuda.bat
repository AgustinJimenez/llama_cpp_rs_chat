@echo off
REM Build script with CUDA support using Visual Studio environment

echo Setting up Visual Studio 2022 Community environment...
call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat"

if errorlevel 1 (
    echo ERROR: Failed to set up Visual Studio environment
    pause
    exit /b 1
)

REM Add CMake to PATH
set "PATH=%PATH%;C:\Program Files\CMake\bin"
echo CMake added to PATH

echo.
echo Building with CUDA support...
cargo build --features cuda --bin llama_chat_web %*

if errorlevel 1 (
    echo.
    echo [ERROR] Build failed
    pause
    exit /b 1
) else (
    echo.
    echo [SUCCESS] Build completed!
)

pause
