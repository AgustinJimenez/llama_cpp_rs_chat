@echo off
REM Development server with CUDA support

echo Setting up Visual Studio 2022 Community environment...
call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat" >nul

if errorlevel 1 (
    echo ERROR: Failed to set up Visual Studio environment
    pause
    exit /b 1
)

REM Add CMake to PATH
set "PATH=%PATH%;C:\Program Files\CMake\bin"

echo Starting development server with CUDA support...
npm run dev:web
