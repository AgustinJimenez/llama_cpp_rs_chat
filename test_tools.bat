@echo off
REM Tool Testing Script - Quick verification without Playwright
REM This script tests all tool endpoints directly via curl

echo ========================================
echo Testing Tool Execution Endpoints
echo ========================================
echo.

set BASE_URL=http://localhost:8000
set TEST_DIR=%~dp0test_data

echo [1/5] Testing read_file tool...
curl -X POST %BASE_URL%/api/tools/execute ^
  -H "Content-Type: application/json" ^
  -d "{\"tool_name\":\"read_file\",\"arguments\":{\"path\":\"%TEST_DIR%\\sample_file.txt\"}}"
echo.
echo.

echo [2/5] Testing write_file tool...
curl -X POST %BASE_URL%/api/tools/execute ^
  -H "Content-Type: application/json" ^
  -d "{\"tool_name\":\"write_file\",\"arguments\":{\"path\":\"%TEST_DIR%\\test_output.txt\",\"content\":\"Test from curl\"}}"
echo.
echo.

echo [3/5] Testing list_directory tool...
curl -X POST %BASE_URL%/api/tools/execute ^
  -H "Content-Type: application/json" ^
  -d "{\"tool_name\":\"list_directory\",\"arguments\":{\"path\":\"%TEST_DIR%\",\"recursive\":false}}"
echo.
echo.

echo [4/5] Testing bash tool (echo)...
curl -X POST %BASE_URL%/api/tools/execute ^
  -H "Content-Type: application/json" ^
  -d "{\"tool_name\":\"bash\",\"arguments\":{\"command\":\"echo Hello from bash tool\"}}"
echo.
echo.

echo [5/5] Testing bash tool (dir)...
curl -X POST %BASE_URL%/api/tools/execute ^
  -H "Content-Type: application/json" ^
  -d "{\"tool_name\":\"bash\",\"arguments\":{\"command\":\"dir %TEST_DIR%\"}}"
echo.
echo.

echo ========================================
echo Tool Testing Complete!
echo ========================================
echo.
echo To run full Playwright tests:
echo   npx playwright test tests/e2e/tool-api.test.ts
echo.
