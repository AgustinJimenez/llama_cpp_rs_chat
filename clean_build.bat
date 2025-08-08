@echo off
REM Clean build script - removes all build artifacts and rebuilds from scratch

echo 🧹 LLM Chat Clean Builder

REM Set up CMake from Visual Studio Build Tools
set "CMAKE_DIR=C:\Program Files\CMake\bin"
set "PATH=%CMAKE_DIR%;%PATH%"

echo 🗑️  Cleaning previous build artifacts...
if exist target (
    echo    Removing target directory...
    rmdir /s /q target
)

if exist Cargo.lock (
    echo    Removing Cargo.lock...
    del Cargo.lock
)

echo ✅ Clean completed

echo.
echo 🔨 Starting fresh build...
call build.bat %1

echo.
echo 🎉 Clean build process completed!