param(
    [string]$ExePath = (Join-Path $PSScriptRoot '..\target\x86_64-pc-windows-gnu\release\roadwaves.exe')
)

$ErrorActionPreference = 'Stop'
$resolvedExe = (Resolve-Path -LiteralPath $ExePath).Path
$logBase = Join-Path ([IO.Path]::GetTempPath()) ("roadwaves-no-fixture-{0}" -f $PID)
$stdoutPath = "$logBase.out.log"
$stderrPath = "$logBase.err.log"
$process = $null

try {
    $process = Start-Process -FilePath $resolvedExe -ArgumentList @('service', 'run', '--control-token', 'no-fixture-smoke') `
        -RedirectStandardOutput $stdoutPath -RedirectStandardError $stderrPath -PassThru -WindowStyle Hidden
    $ready = $false
    for ($attempt = 0; $attempt -lt 40; $attempt++) {
        try {
            $health = Invoke-RestMethod -Uri 'http://127.0.0.1:47926/health' -Method Get -TimeoutSec 1
            $ready = $health.ok
            if ($ready) { break }
        } catch {
            if ($process.HasExited) { throw "service exited early; see $stderrPath" }
            Start-Sleep -Milliseconds 250
        }
    }
    if (-not $ready) { throw 'normal service did not become ready' }

    $statusCode = 0
    try {
        Invoke-WebRequest -Uri 'http://127.0.0.1:47926/radio.mp3' -Method Get -TimeoutSec 3 | Out-Null
    } catch {
        $statusCode = $_.Exception.Response.StatusCode.value__
    }
    if ($statusCode -ne 503) {
        throw "normal service returned unexpected audio status $statusCode"
    }
    $status = Invoke-RestMethod -Uri 'http://127.0.0.1:47926/status' -Method Get -TimeoutSec 3
    if ([string]::IsNullOrWhiteSpace($status.provider_error)) {
        throw 'normal service did not expose its no-live-stream reason in provider_error'
    }
    Write-Output ('synthetic_audio_guard=passed status=503 provider_error={0}' -f $status.provider_error)
} finally {
    if ($process -and -not $process.HasExited) {
        Stop-Process -Id $process.Id -Force
        $process.WaitForExit()
    }
    Remove-Item -LiteralPath $stdoutPath, $stderrPath -Force -ErrorAction SilentlyContinue
}
