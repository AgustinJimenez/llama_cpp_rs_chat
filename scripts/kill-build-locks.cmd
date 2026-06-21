@echo off
setlocal enabledelayedexpansion

:: Kill any process whose .exe lives inside target\ (release or debug builds)
:: These hold file locks that cause "Acceso denegado" (Access Denied) on rebuild.
::
:: Uses PowerShell to find running processes by executable path matching %CD%\target\

echo [kill-build-locks] Checking for stale build processes...

powershell -NoProfile -Command ^
    "$targetDir = '%CD:\=\\%\\target'; " ^
    "Get-Process | Where-Object { $_.MainModule.FileName -like \"$targetDir*\" } 2>$null | ForEach-Object { " ^
    "  Write-Host (\"  Killing \" + $_.Name + \".exe (PID \" + $_.Id + \")\"); " ^
    "  Stop-Process -Id $_.Id -Force -ErrorAction SilentlyContinue " ^
    "}"

echo [kill-build-locks] Done.
