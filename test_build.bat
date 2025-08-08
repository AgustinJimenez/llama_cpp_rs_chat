@echo off
REM Test script to verify build works correctly

echo 🧪 LLM Chat Build Test

REM Build the project first
call build.bat

if errorlevel 1 (
    echo ❌ Build failed, cannot run tests
    pause
    exit /b 1
)

echo.
echo 🔍 Testing binary execution (should show help/usage)...
echo.

REM Test that the binary can start (will show model prompt, then we'll Ctrl+C)
echo 💡 This will start the application briefly to test it works
echo    Press Ctrl+C when you see the model path prompt
echo.
pause

target\debug\main.exe

echo.
echo ✅ Build test completed
echo    If you saw the model path prompt, the build is working correctly

pause