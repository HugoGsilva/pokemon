# PokeGrid Remoto no PC: mantém o servidor rodando (volta sozinho se cair) e se atualiza quando sai versão nova
# em Releases (remoto-pc), que o GitHub Actions publica a cada push. O pokegrid.conf e a pasta dados ficam.
$ErrorActionPreference = "Stop"
$dir = $PSScriptRoot
$server = Join-Path $dir "pokegrid-server.exe"
$versionFile = Join-Path $dir "versao.txt"
$stopFile = Join-Path $dir "parar"
$api = "https://api.github.com/repos/HugoGsilva/pokemon/releases/tags/remoto-pc"
$checkEvery = [TimeSpan]::FromMinutes(15)

function Log($msg) { Write-Host "[atualizar] $msg" }

function Get-Latest {
    try {
        $r = Invoke-RestMethod $api -Headers @{ "User-Agent" = "PokeGrid-Remoto" } -TimeoutSec 20 -UseBasicParsing
        $asset = $r.assets | Where-Object name -eq "PokeGridRemoto-PC.zip" | Select-Object -First 1
        if ($asset) { return [pscustomobject]@{ Version = $r.target_commitish; Url = $asset.browser_download_url } }
    } catch {
        Log "sem acesso ao GitHub: $($_.Exception.Message)"
    }
    return $null
}

function Get-Current {
    if (Test-Path $versionFile) { return (Get-Content $versionFile -Raw).Trim() }
    return ""
}

function Install-Update($latest) {
    $tmp = Join-Path $env:TEMP "pokegrid-atualizacao"
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
    New-Item -ItemType Directory $tmp | Out-Null
    $zip = Join-Path $tmp "pacote.zip"
    Invoke-WebRequest $latest.Url -OutFile $zip -UseBasicParsing
    $x = Join-Path $tmp "x"
    Expand-Archive $zip -DestinationPath $x -Force
    # O iniciar.bat não é trocado com a janela aberta (o cmd lê o .bat enquanto roda)
    foreach ($f in "pokegrid-server.exe", "chisel.exe", "pokegrid.ps1", "pokegrid.conf.example") {
        if (Test-Path "$x\$f") { Copy-Item "$x\$f" $dir -Force }
    }
    Remove-Item -Recurse -Force (Join-Path $dir "web") -ErrorAction SilentlyContinue
    Copy-Item -Recurse "$x\web" (Join-Path $dir "web")
    Set-Content $versionFile $latest.Version -Encoding Ascii
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
    Log "versão $($latest.Version.Substring(0, 7)) instalada"
}

function Update-IfNew($latest) {
    if ($latest -and $latest.Version -and $latest.Version -ne (Get-Current)) {
        Log "versão nova $($latest.Version.Substring(0, 7))"
        try { Install-Update $latest } catch { Log "falhou: $($_.Exception.Message)" }
    }
}

# Desliga o servidor com calma (ele grava o histórico ao ver o arquivo "parar"); depois de 20 s, à força
function Stop-Server($p) {
    New-Item -ItemType File $stopFile -Force | Out-Null
    if (-not $p.WaitForExit(20000)) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue; $p.WaitForExit() }
    Remove-Item $stopFile -Force -ErrorAction SilentlyContinue
}

Remove-Item $stopFile -Force -ErrorAction SilentlyContinue
while ($true) {
    Update-IfNew (Get-Latest)
    $p = Start-Process -FilePath $server -WorkingDirectory $dir -NoNewWindow -PassThru
    $next = (Get-Date) + $checkEvery
    $updating = $false
    while (-not $p.HasExited) {
        Start-Sleep -Seconds 5
        if ((Get-Date) -lt $next) { continue }
        $next = (Get-Date) + $checkEvery
        $latest = Get-Latest
        if ($latest -and $latest.Version -and $latest.Version -ne (Get-Current)) {
            Log "versão nova no GitHub: reiniciando as contas para atualizar"
            Stop-Server $p
            $updating = $true
        }
    }
    if (-not $updating) {
        Write-Host "[servidor] saiu (código $($p.ExitCode)); de novo em 5 s"
        Start-Sleep -Seconds 5
    }
}
