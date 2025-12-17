$ErrorActionPreference = "Stop"

function Ensure-Dir([string]$Path) {
  if (!(Test-Path -LiteralPath $Path)) {
    New-Item -ItemType Directory -Path $Path | Out-Null
  }
}

function Stop-ListenerOnPort([int]$Port) {
  $conn = Get-NetTCPConnection -LocalPort $Port -State Listen -ErrorAction SilentlyContinue | Select-Object -First 1
  if ($null -ne $conn) {
    $procId = $conn.OwningProcess
    if ($procId -and ($procId -ne $PID)) {
      try {
        Stop-Process -Id $procId -Force -ErrorAction Stop
        Write-Host "Stopped process $procId listening on port $Port"
      } catch {
        Write-Host "Failed to stop process $procId on port ${Port}: $($_.Exception.Message)"
      }
    }
  }
}

$repoRoot = Split-Path -Parent $PSScriptRoot
$logDir = Join-Path $repoRoot "logs/dev"
Ensure-Dir $logDir

Stop-ListenerOnPort 8000
Stop-ListenerOnPort 4000

$stamp = Get-Date -Format "yyyy-MM-dd-HH_mm_ss"
$backendOut = Join-Path $logDir "backend_$stamp.out.log"
$backendErr = Join-Path $logDir "backend_$stamp.err.log"
$frontendOut = Join-Path $logDir "frontend_$stamp.out.log"
$frontendErr = Join-Path $logDir "frontend_$stamp.err.log"

$backend = Start-Process `
  -FilePath "cargo" `
  -ArgumentList @("run", "--bin", "llama_chat_web") `
  -WorkingDirectory $repoRoot `
  -WindowStyle Hidden `
  -RedirectStandardOutput $backendOut `
  -RedirectStandardError $backendErr `
  -PassThru

$frontend = Start-Process `
  -FilePath "cmd.exe" `
  -ArgumentList @("/c", "npx", "vite", "--host", "--port", "4000") `
  -WorkingDirectory $repoRoot `
  -WindowStyle Hidden `
  -RedirectStandardOutput $frontendOut `
  -RedirectStandardError $frontendErr `
  -PassThru

$pids = @{
  backend = $backend.Id
  frontend = $frontend.Id
  started_at = (Get-Date).ToString("o")
  logs = @{
    backend_out = $backendOut
    backend_err = $backendErr
    frontend_out = $frontendOut
    frontend_err = $frontendErr
  }
}

$pidsPath = Join-Path $logDir "pids.json"
$pids | ConvertTo-Json -Depth 5 | Set-Content -Path $pidsPath -Encoding UTF8

Write-Host "Started backend PID $($backend.Id) (port 8000)"
Write-Host "Started frontend PID $($frontend.Id) (port 4000)"
Write-Host "PID file: $pidsPath"
