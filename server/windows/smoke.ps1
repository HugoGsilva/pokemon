# Teste do servidor no ar (usado pelo GitHub Actions): /api/info com todas as contas rodando e com RAM medida,
# a página e pelo menos um quadro JPEG de cada conta pelo WebSocket.
param(
    [string]$Base = "http://127.0.0.1:8080",
    [string]$User = "pokegrid",
    [string]$Password = "",
    [int]$Accounts = 2,
    [int]$TimeoutSec = 120
)
$ErrorActionPreference = "Stop"
$headers = @{}
if ($Password) {
    $b64 = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes("${User}:${Password}"))
    $headers["Authorization"] = "Basic $b64"
}

$deadline = (Get-Date).AddSeconds($TimeoutSec)
$info = $null
while ((Get-Date) -lt $deadline) {
    try {
        $info = Invoke-RestMethod -Uri "$Base/api/info" -Headers $headers -TimeoutSec 10
        $ready = @($info.accounts | Where-Object { $_.state -eq "running" -and $_.memMb -gt 0 })
        if ($ready.Count -eq $Accounts) { break }
    } catch { }
    Start-Sleep -Seconds 3
}
if (-not $info) { throw "sem resposta de $Base/api/info" }
$info.accounts | Format-Table slot, state, memMb, restarts, url, error | Out-String | Write-Host
$ready = @($info.accounts | Where-Object { $_.state -eq "running" -and $_.memMb -gt 0 })
if ($ready.Count -ne $Accounts) { throw "contas rodando com RAM medida: $($ready.Count) de $Accounts" }

$page = Invoke-WebRequest -Uri "$Base/" -Headers $headers -UseBasicParsing -TimeoutSec 20
if ($page.Content -notmatch "screens") { throw "a página não veio" }

$ws = [System.Net.WebSockets.ClientWebSocket]::new()
foreach ($k in $headers.Keys) { $ws.Options.SetRequestHeader($k, $headers[$k]) }
$uri = [Uri](($Base -replace "^http", "ws") + "/ws")
$ws.ConnectAsync($uri, [Threading.CancellationToken]::None).GetAwaiter().GetResult()
$slots = (1..$Accounts) -join ","
$msg = [Text.Encoding]::UTF8.GetBytes("{""t"":""view"",""slots"":[$slots],""focus"":0}")
$ws.SendAsync([ArraySegment[byte]]::new($msg), "Text", $true, [Threading.CancellationToken]::None).GetAwaiter().GetResult()

$seen = @{}
$buf = New-Object byte[] 1048576
$frameDeadline = (Get-Date).AddSeconds(60)
while ($seen.Count -lt $Accounts -and (Get-Date) -lt $frameDeadline) {
    $cts = [Threading.CancellationTokenSource]::new(15000)
    $len = 0
    do {
        $r = $ws.ReceiveAsync([ArraySegment[byte]]::new($buf, $len, $buf.Length - $len), $cts.Token).GetAwaiter().GetResult()
        $len += $r.Count
    } while (-not $r.EndOfMessage)
    if ($r.MessageType -eq "Binary" -and $len -gt 100 -and $buf[1] -eq 0xFF -and $buf[2] -eq 0xD8) {
        $seen[[int]$buf[0]] = $len
    }
}
$ws.Dispose()
$seen.GetEnumerator() | ForEach-Object { Write-Host "quadro da conta $($_.Key): $($_.Value) bytes" }
if ($seen.Count -lt $Accounts) { throw "quadros recebidos de $($seen.Count) de $Accounts contas" }
Write-Host "OK: $Base"
