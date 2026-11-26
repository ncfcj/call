# Teste da etapa 4b: servidores, canais e chat em tempo real no app desktop.
#
# Uso (com a API rodando; por padrão a local):
#   powershell -ExecutionPolicy Bypass -File scripts\teste-desktop-chat.ps1
#
# O script abre o app, pede o código de convite de um servidor seu e põe o robô do
# teste-client.ps1 para conversar com você nele. No fim, pergunta o que você viu.

param([string]$Api = "http://127.0.0.1:3000")

$ErrorActionPreference = "Stop"
. "$PSScriptRoot\comum.ps1"

$health = Invoke-Api GET "/health"
if ($health.Status -ne 200) { throw "API fora do ar em $Api" }
Write-Host "Compilando o app (a primeira vez demora)..."
cargo build -q -p call-desktop
$env:CALL_API_URL = $Api
$app = Start-Process cargo -ArgumentList "run", "-q", "-p", "call-desktop" -PassThru -NoNewWindow

Write-Host ""
Write-Host "1. No app, entre e crie um servidor pelo '+' da barra da esquerda." -ForegroundColor Cyan
Write-Host "   O canal # geral abre sozinho e o rodapé mostra 'Conectado'."
Write-Host "2. Clique em 'copiar', ao lado do convite, e cole o código aqui."
$convite = Read-Host "Código de convite"

Write-Host ""
Write-Host "3. O robô vai entrar, digitar e mandar uma mensagem. Responda algumas vezes." -ForegroundColor Cyan
Write-Host "   Crie também um canal de texto e fique nele enquanto o robô responde no # geral."
& "$PSScriptRoot\teste-client.ps1" -Convite $convite -Api $Api -Minutos 2

$itens = @(
    "o robô apareceu online na lista de membros e foi para offline quando saiu",
    "apareceu 'robo... está digitando...' embaixo das mensagens",
    "a mensagem do robô e os ecos chegaram sem recarregar nada",
    "o # geral ganhou o '•' de não lido enquanto você estava no outro canal"
)
foreach ($item in $itens) {
    $resposta = Read-Host "Confere: $item? (s/n)"
    Test-Step $item ($resposta -eq "s")
}

Write-Host ""
Write-Host "4. Feche o app. Ele abre de novo: deve voltar no mesmo servidor, com o histórico." -ForegroundColor Cyan
$app.WaitForExit()
cargo run -q -p call-desktop
$resposta = Read-Host "Voltou no mesmo servidor, com o histórico? (s/n)"
Test-Step "a sessão e o histórico voltam ao abrir o app" ($resposta -eq "s")

if ($script:falhas -gt 0) { Write-Host "`n$($script:falhas) falha(s)." -ForegroundColor Red; exit 1 }
Write-Host "`nEtapa 4b OK." -ForegroundColor Green
