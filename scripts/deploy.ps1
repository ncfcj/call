# Compila e publica o Call numa VM com Ubuntu 24.04 (x86), por SSH com chave.
# Precisa do Docker ligado (compila o servidor para Linux) e do trunk (compila o client).
#
# Uso:
#   powershell -ExecutionPolicy Bypass -File scripts\deploy.ps1 -Vm ubuntu@IP -Dominio nome.duckdns.org
# Na primeira vez, para depois de criar /etc/call/call.env na VM: preencha e rode de novo.

param(
    # usuario@ip; o usuário precisa ser root ou ter sudo sem senha (o padrão nas nuvens).
    [Parameter(Mandatory)][string]$Vm,
    [Parameter(Mandatory)][ValidatePattern('^[a-z0-9.-]+$')][string]$Dominio,
    # Só compila e monta o pacote, sem enviar.
    [switch]$SoPacote
)

$ErrorActionPreference = "Stop"
$raiz = Split-Path $PSScriptRoot
$pacote = Join-Path $raiz "target\deploy"

function Assert-Ok([string]$Etapa) {
    if ($LASTEXITCODE -ne 0) { throw "$Etapa falhou (código $LASTEXITCODE)" }
}

if (Test-Path $pacote) { Remove-Item $pacote -Recurse -Force -Confirm:$false }
New-Item -ItemType Directory $pacote | Out-Null

Write-Host "Compilando o servidor (Linux)"
docker build --platform linux/amd64 --target binario -f "$raiz\deploy\Dockerfile" --output "type=local,dest=$pacote" $raiz
Assert-Ok "docker build"

Write-Host "Compilando o client"
$antes = $env:CALL_API_URL
$env:CALL_API_URL = "https://api.$Dominio"
Push-Location "$raiz\web"
try {
    trunk build --release --dist "$pacote\web"
    Assert-Ok "trunk build"
} finally {
    Pop-Location
    $env:CALL_API_URL = $antes
}

Copy-Item "$raiz\deploy\Caddyfile", "$raiz\deploy\call-server.service", "$raiz\deploy\call.env.example", "$raiz\deploy\instalar.sh" $pacote
if ($SoPacote) { Write-Host "Pacote em $pacote"; return }

Write-Host "Enviando para $Vm"
ssh $Vm "rm -rf call-deploy"
Assert-Ok "ssh"
scp -q -r $pacote "${Vm}:call-deploy"
Assert-Ok "scp"
ssh $Vm "sudo bash call-deploy/instalar.sh $Dominio"
Assert-Ok "instalar.sh"

$saude = Invoke-RestMethod "https://api.$Dominio/health" -TimeoutSec 60
if (-not $saude.database) { throw "a API respondeu, mas sem banco" }
Write-Host "Tudo certo: https://$Dominio" -ForegroundColor Green
