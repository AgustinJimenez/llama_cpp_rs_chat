@echo off
REM Batch script to add CMake to system PATH permanently
REM Run this as Administrator: Right-click -> "Run as administrator"

echo Checking for CMake installation...
echo.

set "CMAKE_PATH=C:\Program Files\CMake\bin"

if exist "%CMAKE_PATH%\cmake.exe" (
    echo Found CMake at: %CMAKE_PATH%
    echo.
    echo Adding CMake to system PATH...

    REM Add to system PATH using setx
    setx PATH "%PATH%;%CMAKE_PATH%" /M

    if %ERRORLEVEL% EQU 0 (
        echo.
        echo [SUCCESS] CMake successfully added to system PATH!
        echo.
        echo IMPORTANT: You need to restart your terminal/IDE for changes to take effect.
        echo.
        echo After restarting, verify with: cmake --version
    ) else (
        echo.
        echo [ERROR] Failed to update PATH. Make sure you're running as Administrator.
        echo Right-click this file and select "Run as administrator"
    )
) else (
    echo [ERROR] CMake not found at expected location: %CMAKE_PATH%
    echo.
    echo Searching for CMake installations...
    echo.

    if exist "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe" (
        echo Found CMake at: C:\Program Files (x86^)\Microsoft Visual Studio\2022\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin
    )

    if exist "C:\Program Files (x86)\Microsoft Visual Studio\2019\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe" (
        echo Found CMake at: C:\Program Files (x86^)\Microsoft Visual Studio\2019\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin
    )
)

echo.
pause
