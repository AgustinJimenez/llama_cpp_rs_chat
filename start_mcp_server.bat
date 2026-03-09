@echo off
REM Start MCP Desktop Tools server (HTTP/SSE mode)
REM Run this before starting Claude Code to enable desktop automation tools.
REM The server listens on http://localhost:18090/mcp
start /B "" "%~dp0target\release\mcp_desktop_tools.exe" --http 18090
echo MCP Desktop Tools server started on http://localhost:18090/mcp
echo Press Ctrl+C to stop.
