param(
    [string]$ExePath = (Join-Path $PSScriptRoot '..\target\x86_64-pc-windows-gnu\release\roadwaves.exe'),
    [string]$ControlToken = 'roadwaves-live-smoke-token'
)

$ErrorActionPreference = 'Stop'
$resolvedExe = (Resolve-Path -LiteralPath $ExePath).Path
$logBase = Join-Path ([IO.Path]::GetTempPath()) ("roadwaves-live-smoke-{0}" -f $PID)
$stdoutPath = "$logBase.out.log"
$stderrPath = "$logBase.err.log"
$process = $null
$udp = $null
$client = $null

function Get-ServiceStatus {
    Invoke-RestMethod -Uri 'http://127.0.0.1:47926/status' -Method Get -TimeoutSec 3
}

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
    $location = Invoke-RestMethod -Uri 'http://127.0.0.1:47926/control' -Method Post -Headers $headers `
        -ContentType 'application/json' -Body '{"action":"location","latitude":52.52,"longitude":13.405}'
    if (-not $location.ok) { throw 'manual location was not acknowledged' }

    $udp = [System.Net.Sockets.UdpClient]::new()
    $packet = '{"protocol_version":1,"game":"ets2","session_id":"live-smoke","sequence":1,"timestamp_unix_ms":1700000000000,"world":{"x":0.0,"y":0.0,"z":0.0},"heading":null,"paused":false}'
    $bytes = [Text.Encoding]::UTF8.GetBytes($packet)
    [void]$udp.Send($bytes, $bytes.Length, '127.0.0.1', 47927)

    $status = $null
    for ($attempt = 0; $attempt -lt 60; $attempt++) {
        $status = Get-ServiceStatus
        if ($status.current_channel_id) { break }
        if ($process.HasExited) { throw "service exited during discovery; see $stderrPath" }
        Start-Sleep -Seconds 1
    }
    if (-not $status.current_channel_id) {
        throw "no live channel selected: $($status.provider_error)"
    }

    $selectedChannelId = $status.current_channel_id
    $hidden = Invoke-RestMethod -Uri 'http://127.0.0.1:47926/control' -Method Post -Headers $headers `
        -ContentType 'application/json' -Body '{"action":"hide"}'
    if (-not $hidden.ok -or $hidden.action -notlike '*hidden=true*') {
        throw "hide control was not acknowledged: $($hidden | ConvertTo-Json -Compress)"
    }
    $unhidden = Invoke-RestMethod -Uri 'http://127.0.0.1:47926/control' -Method Post -Headers $headers `
        -ContentType 'application/json' -Body ((@{ action = 'unhide'; channel_id = $selectedChannelId } | ConvertTo-Json -Compress))
    if (-not $unhidden.ok -or $unhidden.action -notlike '*hidden=false*') {
        throw "unhide control was not acknowledged: $($unhidden | ConvertTo-Json -Compress)"
    }

    $request = [Net.HttpWebRequest]::Create('http://127.0.0.1:47926/radio.mp3')
    $request.Timeout = 15000
    $request.ReadWriteTimeout = 15000
    $response = $request.GetResponse()
    try {
        $stream = $response.GetResponseStream()
        try {
            $audio = New-Object byte[] 4
            $read = $stream.Read($audio, 0, 4)
        } finally { $stream.Dispose() }
    } finally { $response.Dispose() }
    if ($read -lt 2 -or $audio[0] -ne 0xff -or (($audio[1] -band 0xe0) -ne 0xe0)) {
        throw 'live relay did not return an MP3 frame prefix'
    }
    Write-Output ("service_live_smoke=passed channel={0} station={1} audio_bytes={2}" -f $status.current_channel_id, $status.current_station, $read)
} finally {
    if ($udp) { $udp.Dispose() }
    if ($client) { $client.Dispose() }
    if ($process -and -not $process.HasExited) { Stop-Process -Id $process.Id -Force; $process.WaitForExit() }
    Remove-Item -LiteralPath $stdoutPath, $stderrPath -Force -ErrorAction SilentlyContinue
}
