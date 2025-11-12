# Test bash tool with quoted Windows path
$testPath = "E:\repo\llama_cpp_rs_chat\test_data\config.json"
$command = "type `"$testPath`""

Write-Host "Testing bash tool with command: $command"

$body = @{
    tool_name = "bash"
    arguments = @{
        command = $command
    }
} | ConvertTo-Json -Compress

Write-Host "Request body: $body"

try {
    $response = Invoke-RestMethod -Uri "http://localhost:8000/api/tools/execute" `
                                   -Method Post `
                                   -Body $body `
                                   -ContentType "application/json" `
                                   -ErrorAction Stop

    Write-Host "`n=== RESPONSE ===" -ForegroundColor Green
    $response | ConvertTo-Json -Depth 5

    if ($response.success -eq $true) {
        Write-Host "`n✅ SUCCESS - Bash tool with quoted path works!" -ForegroundColor Green
    } else {
        Write-Host "`n❌ FAILED - Command execution failed" -ForegroundColor Red
    }
} catch {
    Write-Host "`n❌ ERROR: $_" -ForegroundColor Red
}
