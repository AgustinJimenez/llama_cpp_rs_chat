@echo off
set "CMAKE_DIR=C:\Program Files\CMake\bin"
set "PATH=%CMAKE_DIR%;%PATH%"
echo Building simple chat version...
cargo build --bin main_minimal
if %ERRORLEVEL% EQU 0 (
    echo Running simple chat...
    cargo run --bin main_minimal
) else (
    echo Build failed!
)
pause