@echo off
setlocal enabledelayedexpansion

:: Detect Visual Studio via vswhere
for /f "usebackq delims=" %%i in (`"C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath`) do (
    set "VSINSTALLPATH=%%i"
)

if defined VSINSTALLPATH (
    set "VCVARS=!VSINSTALLPATH!\VC\Auxiliary\Build\vcvars64.bat"
    if exist "!VCVARS!" (
        call "!VCVARS!"
    )
)

:: Put MSVC link.exe before Git link.exe
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"

endlocal & set "PATH=%PATH%"
