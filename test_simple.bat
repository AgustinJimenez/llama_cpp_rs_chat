@echo off
echo Building simple chat version...
cargo build --bin main_simple
if %ERRORLEVEL% EQU 0 (
    echo Running simple chat...
    cargo run --bin main_simple
) else (
    echo Build failed!
)
pause