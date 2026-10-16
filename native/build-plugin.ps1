param(
    [string]$OutputPath = ""
)

$ErrorActionPreference = "Stop"
$root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$output = if ([string]::IsNullOrWhiteSpace($OutputPath)) {
    Join-Path $root "target\roadwaves_scs.dll"
} else {
    [IO.Path]::GetFullPath($OutputPath)
}
$compiler = if ($env:CXX) {
    $env:CXX
} elseif (Test-Path -LiteralPath "C:\git-sdk-64\ucrt64\bin\g++.exe") {
    "C:\git-sdk-64\ucrt64\bin\g++.exe"
} else {
    "g++"
}
New-Item -ItemType Directory -Force (Split-Path $output) | Out-Null
$compilerPath = (Get-Command $compiler -ErrorAction SilentlyContinue).Source
$compilerDirectory = if ($compilerPath) { Split-Path $compilerPath } elseif (Test-Path -LiteralPath $compiler) { Split-Path $compiler } else { $null }
$oldPath = $env:PATH
try {
    if ($compilerDirectory) { $env:PATH = "$compilerDirectory;$oldPath" }
    & $compiler -std=c++17 -O2 -Wall -Wextra -shared -static -static-libgcc -static-libstdc++ `
      "-Wl,--no-insert-timestamp" `
      "-I$(Join-Path $root 'native\scs_sdk\include')" `
      (Join-Path $root "native\roadwaves_scs.cpp") -o $output -lws2_32
    if ($LASTEXITCODE -ne 0) { throw "SCS plugin compilation failed" }
} finally {
    $env:PATH = $oldPath
}
$output
