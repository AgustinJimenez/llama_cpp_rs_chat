$ErrorActionPreference = "Stop"

function Stop-ProcessId([int]$ProcessId) {
  if (!$ProcessId) { return }
  try {
    $proc = Get-Process -Id $ProcessId -ErrorAction Stop
    Stop-Process -Id $ProcessId -Force -ErrorAction Stop
    Write-Host "Stopped PID $ProcessId ($($proc.ProcessName))"
  } catch {
    Write-Host "PID $ProcessId not running or not stoppable: $($_.Exception.Message)"
  }
}

$repoRoot = Split-Path -Parent $PSScriptRoot
$pidsPath = Join-Path $repoRoot "logs/dev/pids.json"

if (Test-Path -LiteralPath $pidsPath) {
  $data = Get-Content -LiteralPath $pidsPath -Raw | ConvertFrom-Json
  Stop-ProcessId ([int]$data.frontend)
  Stop-ProcessId ([int]$data.backend)
  Remove-Item -LiteralPath $pidsPath -Force -ErrorAction SilentlyContinue
  Write-Host "Removed PID file $pidsPath"
} else {
  Write-Host "No PID file found at $pidsPath"
}
