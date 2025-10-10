# PowerShell script to add CMake to system PATH permanently
# Run this as Administrator: Right-click -> "Run with PowerShell" (as Admin)

$cmakePath = "C:\Program Files\CMake\bin"

# Check if CMake exists at this path
if (Test-Path "$cmakePath\cmake.exe") {
    Write-Host "Found CMake at: $cmakePath" -ForegroundColor Green

    # Get current system PATH
    $currentPath = [Environment]::GetEnvironmentVariable("Path", "Machine")

    # Check if CMake is already in PATH
    if ($currentPath -like "*$cmakePath*") {
        Write-Host "CMake is already in system PATH!" -ForegroundColor Yellow
    } else {
        Write-Host "Adding CMake to system PATH..." -ForegroundColor Cyan

        try {
            # Add CMake to system PATH
            $newPath = "$currentPath;$cmakePath"
            [Environment]::SetEnvironmentVariable("Path", $newPath, "Machine")

            Write-Host "✓ CMake successfully added to system PATH!" -ForegroundColor Green
            Write-Host ""
            Write-Host "IMPORTANT: You need to restart your terminal/IDE for changes to take effect." -ForegroundColor Yellow
            Write-Host ""
            Write-Host "After restarting, verify with: cmake --version" -ForegroundColor Cyan
        } catch {
            Write-Host "✗ Failed to update PATH. Make sure you're running as Administrator." -ForegroundColor Red
            Write-Host "Error: $_" -ForegroundColor Red
        }
    }
} else {
    Write-Host "✗ CMake not found at expected location: $cmakePath" -ForegroundColor Red
    Write-Host "Searching for CMake installations..." -ForegroundColor Cyan

    $possiblePaths = @(
        "C:\Program Files\CMake\bin",
        "C:\Program Files (x86)\CMake\bin",
        "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin",
        "C:\Program Files (x86)\Microsoft Visual Studio\2019\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin"
    )

    foreach ($path in $possiblePaths) {
        if (Test-Path "$path\cmake.exe") {
            Write-Host "Found CMake at: $path" -ForegroundColor Green
        }
    }
}

Write-Host ""
Write-Host "Press any key to exit..."
$null = $Host.UI.RawUI.ReadKey("NoEcho,IncludeKeyDown")
