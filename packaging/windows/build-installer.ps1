$ErrorActionPreference = "Stop"
$repo = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$isccCandidates = @("C:\Program Files (x86)\Inno Setup 6\ISCC.exe", "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe")
$iscc = $isccCandidates | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
if (-not (Test-Path -LiteralPath $iscc)) { throw "Inno Setup 6 is not installed" }
$dist = Join-Path $repo "dist"
New-Item -ItemType Directory -Force -Path $dist | Out-Null
$pluginOutput = Join-Path $repo "target\x86_64-pc-windows-gnu\release\roadwaves_scs.dll"
& (Join-Path $repo "native\build-plugin.ps1") -OutputPath $pluginOutput
if ($LASTEXITCODE -ne 0) { throw "native plugin build failed with exit code $LASTEXITCODE" }
& $iscc "/DRepoDir=$repo" "/DOutputDir=$dist" (Join-Path $PSScriptRoot "roadwaves.iss")
if ($LASTEXITCODE -ne 0) { throw "ISCC failed with exit code $LASTEXITCODE" }
Get-ChildItem -LiteralPath $dist -File
