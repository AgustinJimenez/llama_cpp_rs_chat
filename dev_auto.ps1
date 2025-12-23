# Auto-detection PowerShell script for optimal GPU acceleration on Windows
# Dynamically detects Windows setup, CUDA, Visual Studio, and runs the best configuration
# Usage: ./dev_auto.ps1 [web|desktop]

param(
    [string]$Mode = "web"
)

if ($Mode -notin @("web", "desktop")) {
    Write-Host "❌ Invalid mode: $Mode. Use 'web' or 'desktop'" -ForegroundColor Red
    exit 1
}

Write-Host "🔍 Auto-detecting optimal GPU acceleration setup..." -ForegroundColor Cyan
Write-Host "🎯 Mode: $($Mode.ToUpper())" -ForegroundColor Blue
Write-Host ""

# Get system information
$OS = "Windows"
$Arch = (Get-WmiObject Win32_Processor).Architecture
$ArchName = if ($Arch -eq 9) { "x64" } else { "x86" }

Write-Host "🖥️  Platform: $OS ($ArchName)" -ForegroundColor White

# Initialize variables
$UseCuda = $false
$UseCpu = $false
$Features = ""
$ScriptCmd = ""

# Function to check if command exists
function Test-Command {
    param($Command)
    $null = Get-Command $Command -ErrorAction SilentlyContinue
    return $?
}

# Function to check CUDA installation
function Test-Cuda {
    if (Test-Command "nvcc") {
        try {
            $nvccOutput = & nvcc --version 2>$null | Select-String "release"
            if ($nvccOutput) {
                $version = ($nvccOutput -split "release ")[1] -split "," | Select-Object -First 1
                Write-Host "✅ CUDA Toolkit detected (version $version)" -ForegroundColor Green
                return $true
            }
        } catch {
            # Ignore errors
        }
    }
    Write-Host "❌ CUDA Toolkit not found" -ForegroundColor Red
    return $false
}

# Function to check Visual Studio/MSVC
function Test-VisualStudio {
    # Check for cl.exe in PATH
    if (Test-Command "cl") {
        Write-Host "✅ Visual Studio/MSVC compiler detected (in PATH)" -ForegroundColor Green
        return $true
    }
    
    # Check common Visual Studio installation paths
    $vsPaths = @(
        "${env:ProgramFiles}\Microsoft Visual Studio\2022\Community\VC\Tools\MSVC\*\bin\Hostx64\x64\cl.exe",
        "${env:ProgramFiles}\Microsoft Visual Studio\2022\Professional\VC\Tools\MSVC\*\bin\Hostx64\x64\cl.exe",
        "${env:ProgramFiles}\Microsoft Visual Studio\2022\Enterprise\VC\Tools\MSVC\*\bin\Hostx64\x64\cl.exe",
        "${env:ProgramFiles(x86)}\Microsoft Visual Studio\2019\Community\VC\Tools\MSVC\*\bin\Hostx64\x64\cl.exe"
    )
    
    foreach ($path in $vsPaths) {
        if (Test-Path $path) {
            Write-Host "✅ Visual Studio/MSVC compiler detected" -ForegroundColor Green
            return $true
        }
    }
    
    Write-Host "❌ Visual Studio/MSVC compiler not found" -ForegroundColor Red
    return $false
}

# Function to check NVIDIA GPU
function Test-NvidiaGpu {
    try {
        $gpu = Get-WmiObject Win32_VideoController | Where-Object { $_.Name -like "*NVIDIA*" }
        if ($gpu) {
            Write-Host "✅ NVIDIA GPU detected: $($gpu.Name)" -ForegroundColor Green
            return $true
        }
    } catch {
        # Ignore errors
    }
    Write-Host "❌ NVIDIA GPU not found" -ForegroundColor Yellow
    return $false
}

Write-Host ""
Write-Host "🔍 Checking GPU acceleration options..." -ForegroundColor Cyan

# Check for CUDA setup
$hasNvidiaGpu = Test-NvidiaGpu
$hasCuda = Test-Cuda
$hasVS = Test-VisualStudio

if ($hasNvidiaGpu -and $hasCuda -and $hasVS) {
    $UseCuda = $true
    $Features = "cuda"
    if ($Mode -eq "desktop") {
        $ScriptCmd = "tauri:dev:cuda"
    } else {
        $ScriptCmd = "dev:cuda"
    }
    Write-Host "🚀 Will use CUDA acceleration for optimal performance" -ForegroundColor Green
} elseif ($hasNvidiaGpu) {
    $UseCpu = $true
    if ($Mode -eq "desktop") {
        $ScriptCmd = "tauri:dev"
    } else {
        $ScriptCmd = "dev"
    }
    Write-Host "⚠️  NVIDIA GPU found but CUDA/Visual Studio not properly configured" -ForegroundColor Yellow
    Write-Host "💡 Run 'build_cuda.bat' or install Visual Studio with C++ tools" -ForegroundColor Cyan
} else {
    $UseCpu = $true
    if ($Mode -eq "desktop") {
        $ScriptCmd = "tauri:dev"
    } else {
        $ScriptCmd = "dev"
    }
    Write-Host "🔄 No NVIDIA GPU detected, using CPU mode" -ForegroundColor Yellow
}

Write-Host ""
Write-Host "📊 Configuration Summary:" -ForegroundColor White
Write-Host "   OS: $OS" -ForegroundColor Gray
Write-Host "   Architecture: $ArchName" -ForegroundColor Gray
Write-Host "   NVIDIA GPU: $(if ($hasNvidiaGpu) { "✅ Detected" } else { "❌ Not found" })" -ForegroundColor Gray
Write-Host "   CUDA: $(if ($UseCuda) { "✅ Enabled" } else { "❌ Disabled" })" -ForegroundColor Gray
Write-Host "   CPU Fallback: $(if ($UseCpu) { "✅ Active" } else { "❌ Not needed" })" -ForegroundColor Gray
if ($Features) {
    Write-Host "   Features: $Features" -ForegroundColor Gray
}
Write-Host "   Command: npm run $ScriptCmd" -ForegroundColor Gray
Write-Host ""

# Display performance expectations
if ($UseCuda) {
    Write-Host "🎯 Expected Performance: 10-50x faster than CPU (CUDA GPU acceleration)" -ForegroundColor Green
} else {
    Write-Host "🎯 Expected Performance: CPU-only mode (slower but compatible)" -ForegroundColor Yellow
}

Write-Host ""
Write-Host "🚀 Starting development server with optimal configuration..." -ForegroundColor Cyan
if ($Mode -eq "desktop") {
    Write-Host "🖥️  Desktop App: Native window will open" -ForegroundColor White
    Write-Host "🔧 Backend: Embedded within desktop app" -ForegroundColor White
} else {
    Write-Host "🌐 Frontend: http://localhost:4000" -ForegroundColor White
    Write-Host "🔧 Backend API: http://localhost:8000" -ForegroundColor White
}
Write-Host ""

# Run the optimal command
& npm run $ScriptCmd