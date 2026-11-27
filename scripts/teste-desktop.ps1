# Teste da etapa 4a: o app desktop entra, e a sessão sobrevive a fechar e abrir o app.
#
# Uso (com a API rodando; por padrão a local):
#   powershell -ExecutionPolicy Bypass -File scripts\teste-desktop.ps1
#   powershell -ExecutionPolicy Bypass -File scripts\teste-desktop.ps1 -Api https://call-70hr.onrender.com
#
# O script abre o app três vezes e confere o Cofre de Credenciais do Windows entre uma e outra.

param([string]$Api = "http://127.0.0.1:3000")

$ErrorActionPreference = "Stop"
. "$PSScriptRoot\comum.ps1"

# O app guarda uma credencial genérica com o serviço "Call" e o endereço da API no nome.
function Test-Session { [bool](cmdkey /list | Select-String -SimpleMatch $Api) }

function Open-App([string]$Instrucao) {
    Write-Host ""
    Write-Host $Instrucao -ForegroundColor Cyan
    Write-Host "Feche a janela do app para continuar."
    $env:CALL_API_URL = $Api
    cargo run -q -p call-desktop
}

$health = Invoke-Api GET "/health"
if ($health.Status -ne 200) { throw "API fora do ar em $Api" }
Write-Host "Compilando o app (a primeira vez demora)..."
cargo build -q -p call-desktop

Open-App "1. Crie uma conta (ou entre com uma que já existe). Deve aparecer 'Olá, <seu nome>'."
Test-Step "sessão salva no Cofre de Credenciais" (Test-Session)

Open-App "2. O app deve abrir direto no 'Olá, <seu nome>', sem pedir login. Depois clique em Sair."
Test-Step "Sair apaga a sessão do cofre" (-not (Test-Session))

Open-App "3. Agora o app deve abrir na tela de login."

$resposta = Read-Host "Os três passos apareceram como descrito? (s/n)"
Test-Step "telas conferidas por você" ($resposta -eq "s")

if ($script:falhas -gt 0) { Write-Host "`n$($script:falhas) falha(s)." -ForegroundColor Red; exit 1 }
Write-Host "`nEtapa 4a OK." -ForegroundColor Green
