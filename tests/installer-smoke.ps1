$ErrorActionPreference = "Stop"
$root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$installer = Join-Path $root "dist\RoadWaves-0.1.0-windows-x64.exe"
$installDir = Join-Path ([IO.Path]::GetTempPath()) ("roadwaves-installer-smoke-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Force -Path $installDir | Out-Null
try {
    $process = Start-Process -FilePath $installer -ArgumentList @(
        "/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/DIR=$installDir"
    ) -Wait -PassThru -WindowStyle Hidden
    if ($process.ExitCode -ne 0) { throw "installer exited with $($process.ExitCode)" }

    $required = @(
        "roadwaves.exe",
        "roadwaves-gui.exe",
        "plugins\roadwaves_scs.dll",
        "README.md",
        "docs\IMPLEMENTATION_REPORT.md"
    )
    $missing = @($required | Where-Object { -not (Test-Path -LiteralPath (Join-Path $installDir $_)) })
    if ($missing.Count -gt 0) { throw "installer payload missing: $($missing -join ', ')" }
    $report = Get-Content -Raw -LiteralPath (Join-Path $installDir "docs\IMPLEMENTATION_REPORT.md")
    if ($report -notmatch "Native plugin ABI harness") { throw "updated implementation report was not embedded" }

    $uninstaller = Join-Path $installDir "unins000.exe"
    $uninstall = Start-Process -FilePath $uninstaller -ArgumentList @(
        "/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART"
    ) -Wait -PassThru -WindowStyle Hidden
    if ($uninstall.ExitCode -ne 0) { throw "uninstaller exited with $($uninstall.ExitCode)" }
    $leftovers = @(Get-ChildItem -LiteralPath $installDir -Force -ErrorAction SilentlyContinue)
    if ($leftovers.Count -gt 0) { throw "uninstaller left files: $($leftovers.Name -join ', ')" }
    Write-Output "installer_smoke=passed"
} finally {
    if (Test-Path -LiteralPath $installDir) {
        Remove-Item -LiteralPath $installDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}
