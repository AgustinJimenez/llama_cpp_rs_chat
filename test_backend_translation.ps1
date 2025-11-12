# Backend Translation Layer Manual Test Script
# This script manually tests the backend translation feature with both models

$BASE_URL = "http://localhost:8000"
$TEST_FILE = "E:\repo\llama_cpp_rs_chat\test_data\config.json"
$TEST_DIR = "E:\repo\llama_cpp_rs_chat\test_data"

# Ensure test file exists
if (!(Test-Path $TEST_FILE)) {
    New-Item -ItemType Directory -Force -Path (Split-Path $TEST_FILE)
    @{ version = "1.0"; test = "Backend translation test" } | ConvertTo-Json | Set-Content $TEST_FILE
    Write-Host "Created test file: $TEST_FILE"
}

Write-Host ""
Write-Host "================================================================"
Write-Host "  Backend Translation Layer - Manual Test"
Write-Host "================================================================"
Write-Host ""

# Function to test tool execution
function Test-ToolExecution {
    param(
        [string]$ModelName,
        [string]$ToolName,
        [hashtable]$Arguments,
        [string]$ExpectedContent
    )

    Write-Host "Testing: $ModelName - $ToolName"
    Write-Host "  Arguments: $($Arguments | ConvertTo-Json -Compress)"

    $body = @{
        tool_name = $ToolName
        arguments = $Arguments
    } | ConvertTo-Json

    try {
        $response = Invoke-RestMethod -Uri "$BASE_URL/api/tools/execute" `
                                       -Method Post `
                                       -Body $body `
                                       -ContentType "application/json" `
                                       -ErrorAction Stop

        $resultStr = $response | ConvertTo-Json -Depth 5

        if ($resultStr -match $ExpectedContent) {
            Write-Host "  ✅ SUCCESS: Found expected content '$ExpectedContent'" -ForegroundColor Green
            return $true
        } else {
            Write-Host "  ❌ FAILED: Expected content not found" -ForegroundColor Red
            Write-Host "  Response: $resultStr"
            return $false
        }
    } catch {
        Write-Host "  ❌ ERROR: $_" -ForegroundColor Red
        return $false
    }
}

# Check server health
Write-Host "Checking server health..."
try {
    $health = Invoke-RestMethod -Uri "$BASE_URL/health"
    Write-Host "  ✅ Server is healthy: $($health.service)" -ForegroundColor Green
} catch {
    Write-Host "  ❌ Server is not responding. Start the server first!" -ForegroundColor Red
    exit 1
}

Write-Host ""
Write-Host "================================================================"
Write-Host "  Phase 1: Test with No Model Loaded"
Write-Host "================================================================"
Write-Host ""

Write-Host "Testing bash tool (should work without model)..."
Test-ToolExecution -ModelName "No Model" `
                   -ToolName "bash" `
                   -Arguments @{ command = "echo test" } `
                   -ExpectedContent "test"

Write-Host ""
Write-Host "================================================================"
Write-Host "  Phase 2: Instructions"
Write-Host "================================================================"
Write-Host ""
Write-Host "MANUAL STEPS REQUIRED:"
Write-Host ""
Write-Host "1. Open the web interface at http://localhost:8000"
Write-Host "2. Load Devstral model"
Write-Host "3. Run the Devstral tests below"
Write-Host "4. Load Qwen3 model"
Write-Host "5. Run the Qwen3 tests below"
Write-Host ""
Write-Host "================================================================"
Write-Host "  Devstral Tests (Run after loading Devstral)"
Write-Host "================================================================"
Write-Host ""

Write-Host "# Test 1: Devstral read_file (native)"
Write-Host "Invoke-RestMethod -Uri 'http://localhost:8000/api/tools/execute' -Method Post -Body '{`"tool_name`":`"read_file`",`"arguments`":{`"path`":`"$($TEST_FILE -replace '\\','\\\\')`"}}' -ContentType 'application/json'"
Write-Host ""

Write-Host "# Test 2: Devstral list_directory (native)"
Write-Host "Invoke-RestMethod -Uri 'http://localhost:8000/api/tools/execute' -Method Post -Body '{`"tool_name`":`"list_directory`",`"arguments`":{`"path`":`"$($TEST_DIR -replace '\\','\\\\')`"}}' -ContentType 'application/json'"
Write-Host ""

Write-Host "================================================================"
Write-Host "  Qwen3 Tests (Run after loading Qwen3)"
Write-Host "================================================================"
Write-Host ""

Write-Host "# Test 3: Qwen3 read_file (auto-translated to bash)"
Write-Host "Invoke-RestMethod -Uri 'http://localhost:8000/api/tools/execute' -Method Post -Body '{`"tool_name`":`"read_file`",`"arguments`":{`"path`":`"$($TEST_FILE -replace '\\','\\\\')`"}}' -ContentType 'application/json'"
Write-Host "  💡 Backend should log: [TOOL TRANSLATION] read_file → bash: type ..."
Write-Host ""

Write-Host "# Test 4: Qwen3 list_directory (auto-translated to bash)"
Write-Host "Invoke-RestMethod -Uri 'http://localhost:8000/api/tools/execute' -Method Post -Body '{`"tool_name`":`"list_directory`",`"arguments`":{`"path`":`"$($TEST_DIR -replace '\\','\\\\')`"}}' -ContentType 'application/json'"
Write-Host "  💡 Backend should log: [TOOL TRANSLATION] list_directory → bash: dir ..."
Write-Host ""

Write-Host "================================================================"
Write-Host "  Expected Results"
Write-Host "================================================================"
Write-Host ""
Write-Host "✅ Devstral:"
Write-Host "   - read_file and list_directory work natively"
Write-Host "   - No [TOOL TRANSLATION] logs"
Write-Host ""
Write-Host "✅ Qwen3:"
Write-Host "   - read_file and list_directory return same results as Devstral"
Write-Host "   - Backend logs show [TOOL TRANSLATION] messages"
Write-Host "   - Transparent to the user!"
Write-Host ""
Write-Host "================================================================"
Write-Host ""
Write-Host "Watch the server console output for [TOOL TRANSLATION] logs!"
Write-Host ""
