#!/bin/bash
# Development script that runs both frontend and backend

export CMAKE="/c/Program Files/CMake/bin/cmake.exe"
export PATH="$PATH:/c/Program Files/CMake/bin"

# Set up Visual Studio 2022 Community environment
export VSINSTALLDIR="/c/Program Files/Microsoft Visual Studio/2022/Community/"
export VCINSTALLDIR="/c/Program Files/Microsoft Visual Studio/2022/Community/VC/"
export VCToolsInstallDir="/c/Program Files/Microsoft Visual Studio/2022/Community/VC/Tools/MSVC/14.44.35207/"

# Add MSVC and Windows SDK to PATH
export PATH="$VCToolsInstallDir/bin/Hostx64/x64:$PATH"
export PATH="/c/Program Files (x86)/Windows Kits/10/bin/10.0.26100.0/x64:$PATH"

# Set up library paths for linking (Windows SDK and VC libs)
export LIB="C:\\Program Files\\Microsoft Visual Studio\\2022\\Community\\VC\\Tools\\MSVC\\14.44.35207\\lib\\x64"
export LIB="$LIB;C:\\Program Files (x86)\\Windows Kits\\10\\Lib\\10.0.26100.0\\um\\x64"
export LIB="$LIB;C:\\Program Files (x86)\\Windows Kits\\10\\Lib\\10.0.26100.0\\ucrt\\x64"

# Set up include paths
export INCLUDE="C:\\Program Files\\Microsoft Visual Studio\\2022\\Community\\VC\\Tools\\MSVC\\14.44.35207\\include"
export INCLUDE="$INCLUDE;C:\\Program Files (x86)\\Windows Kits\\10\\Include\\10.0.26100.0\\ucrt"
export INCLUDE="$INCLUDE;C:\\Program Files (x86)\\Windows Kits\\10\\Include\\10.0.26100.0\\um"
export INCLUDE="$INCLUDE;C:\\Program Files (x86)\\Windows Kits\\10\\Include\\10.0.26100.0\\shared"

echo "Starting development servers with CMake and CUDA support"
npm run dev:web
