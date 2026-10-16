param(
    [string]$ExePath = (Join-Path $PSScriptRoot '..\target\x86_64-pc-windows-gnu\release\roadwaves.exe'),
    [string]$ControlToken = 'roadwaves-smoke-token'
)

$ErrorActionPreference = 'Stop'
$resolvedExe = (Resolve-Path -LiteralPath $ExePath).Path
$logBase = Join-Path ([IO.Path]::GetTempPath()) ("roadwaves-service-smoke-{0}" -f $PID)
$stdoutPath = "$logBase.out.log"
$stderrPath = "$logBase.err.log"
$process = $null
$udp = $null
$webClient = $null

function Get-ServiceStatus {
    Invoke-RestMethod -Uri 'http://127.0.0.1:47926/status' -Method Get -TimeoutSec 3
}

try {
    $process = Start-Process -FilePath $resolvedExe -ArgumentList @('service', 'run', '--fixture', '--control-token', $ControlToken) `
        -RedirectStandardOutput $stdoutPath -RedirectStandardError $stderrPath -PassThru -WindowStyle Hidden

    $health = $null
    for ($attempt = 0; $attempt -lt 40; $attempt++) {
        try {
            $health = Invoke-RestMethod -Uri 'http://127.0.0.1:47926/health' -Method Get -TimeoutSec 1
            break
        } catch {
            if ($process.HasExited) { throw "service exited early; see $stderrPath" }
            Start-Sleep -Milliseconds 250
        }
    }
    if (-not $health.ok) { throw 'health endpoint did not return ok=true' }

    $webClient = New-Object System.Net.WebClient
    $audio = $webClient.DownloadData('http://127.0.0.1:47926/radio.mp3')
    if ($audio.Length -lt 2 -or $audio[0] -ne 0xff -or ($audio[1] -band 0xe0) -ne 0xe0) {
        throw 'audio endpoint did not return an MP3 frame prefix'
    }

    $udp = [System.Net.Sockets.UdpClient]::new()
    $packet = '{"protocol_version":1,"game":"ets2","session_id":"service-smoke","sequence":1,"timestamp_unix_ms":1700000000000,"world":{"x":0.0,"y":0.0,"z":0.0},"heading":null,"paused":false}'
    $packetBytes = [Text.Encoding]::UTF8.GetBytes($packet)
    [void]$udp.Send($packetBytes, $packetBytes.Length, '127.0.0.1', 47927)

    $controlHeaders = @{ 'X-RoadWaves-Token' = $ControlToken }
    $location = Invoke-RestMethod -Uri 'http://127.0.0.1:47926/control' -Method Post `
        -Headers $controlHeaders -ContentType 'application/json' -Body '{"action":"location","latitude":52.52,"longitude":13.405}'
    if (-not $location.ok) { throw 'authenticated location control was not acknowledged' }

    $badTokenRejected = $false
    try {
        Invoke-WebRequest -Uri 'http://127.0.0.1:47926/control' -Method Post `
            -Headers @{ 'X-RoadWaves-Token' = 'wrong-token' } -ContentType 'application/json' -Body '{"action":"refresh"}' | Out-Null
    } catch {
        $badTokenRejected = $_.Exception.Response.StatusCode.value__ -eq 403
    }
    if (-not $badTokenRejected) { throw 'invalid control token was not rejected with HTTP 403' }

    $malformedUnhideRejected = $false
    try {
        Invoke-WebRequest -Uri 'http://127.0.0.1:47926/control' -Method Post `
            -Headers $controlHeaders -ContentType 'application/json' -Body '{"action":"unhide"}' | Out-Null
    } catch {
        $malformedUnhideRejected = $_.Exception.Response.StatusCode.value__ -eq 400
    }
    if (-not $malformedUnhideRejected) { throw 'malformed unhide control was not rejected with HTTP 400' }

    $status = $null
    for ($attempt = 0; $attempt -lt 20; $attempt++) {
        $status = Get-ServiceStatus
        if ($status.last_sequence -eq 1 -and $status.confidence -eq 'ManualOverride' -and $null -ne $status.location) { break }
        Start-Sleep -Milliseconds 100
    }
    if ($status.last_sequence -ne 1) { throw 'telemetry packet did not reach service status' }
    if ($status.confidence -ne 'ManualOverride') { throw 'manual location control did not update confidence' }

    $cliStatusJson = & $resolvedExe service status --json
    if ($LASTEXITCODE -ne 0) { throw 'CLI service status command failed' }
    $cliStatus = $cliStatusJson | ConvertFrom-Json
    if ($cliStatus.last_sequence -ne 1) { throw 'CLI service status did not report telemetry sequence' }

    Write-Output ('service_smoke=passed health=ok audio_bytes={0} sequence={1} confidence={2} cli_status=ok' -f $audio.Length, $status.last_sequence, $status.confidence)
} finally {
    if ($udp) { $udp.Dispose() }
    if ($webClient) { $webClient.Dispose() }
    if ($process -and -not $process.HasExited) {
        Stop-Process -Id $process.Id -Force
        $process.WaitForExit()
    }
    Remove-Item -LiteralPath $stdoutPath, $stderrPath -Force -ErrorAction SilentlyContinue
}
