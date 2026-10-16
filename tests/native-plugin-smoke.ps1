$ErrorActionPreference = "Stop"
$root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$compiler = "C:\git-sdk-64\ucrt64\bin\g++.exe"
$runtime = "C:\git-sdk-64\ucrt64\bin"
if (-not (Test-Path -LiteralPath $compiler)) { throw "g++ not found: $compiler" }
$harness = Join-Path $root "target\native_plugin_harness.exe"
New-Item -ItemType Directory -Force (Split-Path $harness) | Out-Null
$dll = Join-Path $root "target\x86_64-pc-windows-gnu\release\roadwaves_scs.dll"
if (-not (Test-Path -LiteralPath $dll)) { throw "release plugin not found: $dll" }
$oldPath = $env:PATH
try {
    $env:PATH = "$runtime;$oldPath"
    & $compiler -std=c++17 -O2 -Wall -Wextra `
        "-I$(Join-Path $root 'native\scs_sdk\include')" `
        (Join-Path $PSScriptRoot "native_plugin_harness.cpp") `
        -o $harness -lws2_32
    if ($LASTEXITCODE -ne 0) { throw "native harness compilation failed" }
    & $harness $dll
    if ($LASTEXITCODE -ne 0) { throw "native plugin harness failed" }
} finally {
    $env:PATH = $oldPath
}
