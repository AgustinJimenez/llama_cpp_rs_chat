@echo off
set RUSTC=%1
shift

where sccache >nul 2>&1
if %errorlevel%==0 (
  sccache %RUSTC% %*
  exit /b %errorlevel%
)

%RUSTC% %*
exit /b %errorlevel%
