param(
    [string]$ExePath = (Join-Path $PSScriptRoot '..\target\x86_64-pc-windows-gnu\release\roadwaves.exe'),
    [string]$ControlToken = 'roadwaves-default-map-smoke-token'
)

$ErrorActionPreference = 'Stop'
$resolvedExe = (Resolve-Path -LiteralPath $ExePath).Path
$logBase = Join-Path ([IO.Path]::GetTempPath()) ("roadwaves-default-map-smoke-{0}" -f $PID)
$stdoutPath = "$logBase.out.log"
$stderrPath = "$logBase.err.log"
$process = $null
$udp = $null

try {
    $process = Start-Process -FilePath $resolvedExe -ArgumentList @('service', 'run', '--control-token', $ControlToken) `
        -RedirectStandardOutput $stdoutPath -RedirectStandardError $stderrPath -PassThru -WindowStyle Hidden
    for ($attempt = 0; $attempt -lt 40; $attempt++) {
        try { Invoke-RestMethod -Uri 'http://127.0.0.1:47926/health' -TimeoutSec 1 | Out-Null; break }
        catch {
            if ($process.HasExited) { throw "service exited early; see $stderrPath" }
            Start-Sleep -Milliseconds 250
        }
        if ($attempt -eq 39) { throw 'service did not become healthy' }
    }

    $headers = @{ 'X-RoadWaves-Token' = $ControlToken }
    $clear = Invoke-RestMethod -Uri 'http://127.0.0.1:47926/control' -Method Post -Headers $headers `
        -ContentType 'application/json' -Body '{"action":"clear_location"}'
    if (-not $clear.ok) { throw 'could not clear manual location override' }

    $udp = [System.Net.Sockets.UdpClient]::new()
    $packet = '{"protocol_version":1,"game":"ets2","session_id":"default-map-smoke","sequence":1,"timestamp_unix_ms":1700000000000,"world":{"x":-28842.38,"y":46.25,"z":4982.94},"heading":null,"paused":false}'
    $bytes = [Text.Encoding]::UTF8.GetBytes($packet)
    [void]$udp.Send($bytes, $bytes.Length, '127.0.0.1', 47927)

    $status = $null
    for ($attempt = 0; $attempt -lt 60; $attempt++) {
        $status = Invoke-RestMethod -Uri 'http://127.0.0.1:47926/status' -TimeoutSec 3
        if ($status.current_channel_id) { break }
        if ($process.HasExited) { throw "service exited during discovery; see $stderrPath" }
        Start-Sleep -Seconds 1
    }
    if ($status.confidence -ne 'BuiltInEts2Map') {
        throw "stock map transform was not used: confidence=$($status.confidence) location=$($status.location | ConvertTo-Json -Compress)"
    }
    if (-not $status.current_channel_id) {
        throw "no live channel selected: $($status.provider_error)"
    }
    Write-Output ("service_default_map_smoke=passed confidence={0} location={1} channel={2}" -f `
        $status.confidence, ($status.location | ConvertTo-Json -Compress), $status.current_channel_id)
} finally {
    if ($udp) { $udp.Dispose() }
    if ($process -and -not $process.HasExited) { Stop-Process -Id $process.Id -Force; $process.WaitForExit() }
    Remove-Item -LiteralPath $stdoutPath, $stderrPath -Force -ErrorAction SilentlyContinue
}
