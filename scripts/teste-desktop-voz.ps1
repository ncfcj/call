# Teste da etapa 4c: voz no app desktop, numa chamada com a web em outra rede.
#
# Uso (por padrão contra a produção, que tem o LiveKit):
#   powershell -ExecutionPolicy Bypass -File scripts\teste-desktop-voz.ps1
#
# Precisa de uma segunda pessoa (ou de você mesmo) na web, de preferência no celular pelos
# dados móveis, para a chamada passar por redes diferentes. No fim, o script pergunta o que
# você viu e ouviu.

param([string]$Api = "https://call-70hr.onrender.com")

$ErrorActionPreference = "Stop"
. "$PSScriptRoot\comum.ps1"

Write-Host "Acordando a API (o Render free pode levar um minuto)..."
$health = Invoke-Api GET "/health"
if ($health.Status -ne 200) { throw "API fora do ar em $Api" }
Write-Host "Compilando o app (a primeira vez demora: baixa o libwebrtc)..."
cargo build -q -p call-desktop
if ($LASTEXITCODE -ne 0) { throw "o app não compilou" }
$env:CALL_API_URL = $Api
$exe = Join-Path (cargo metadata -q --format-version 1 --no-deps | ConvertFrom-Json).target_directory "debug\call-desktop.exe"
$app = Start-Process $exe -PassThru
Write-Host "O app abriu na janela 'Call' (veja na barra de tarefas se ficou atrás desta)."

Write-Host ""
Write-Host "1. No app, entre e escolha um servidor com um canal de voz (o 'Geral' já vem criado)." -ForegroundColor Cyan
Write-Host "2. Na web ($Api), de outra rede (celular nos dados móveis), entre com outra conta"
Write-Host "   no mesmo servidor e clique no canal de voz."
Write-Host "3. No app, clique no canal de voz (o nome com '>' em CANAIS DE VOZ)."
Write-Host "   Embaixo, o painel deve mostrar 'Voz conectada'. Fale e escute dos dois lados."
Read-Host "Enter quando os dois estiverem na chamada"

$itens = @(
    "os dois se ouvem, sem eco nem microfonia",
    "quem fala aparece em verde e negrito na lista do canal de voz, no app",
    "'Microfone' no app vira 'Mudo', a web para de ouvir você e mostra o microfone fechado",
    "'Som' no app vira 'Sem som' e você deixa de ouvir a web; clicando de novo, volta",
    "em 'Áudio', trocar o microfone ou a saída vale na hora, durante a chamada",
    "'Sair da voz' tira você da chamada e a web deixa de mostrar você no canal"
)
foreach ($item in $itens) {
    $resposta = Read-Host "Confere: $($item)? (s/n)"
    Test-Step $item ($resposta -eq "s")
}

Write-Host ""
Write-Host "4. Entre na voz de novo e feche o app pelo X. A web deve tirar você do canal em até meio minuto." -ForegroundColor Cyan
$app.WaitForExit()
$resposta = Read-Host "A web tirou você do canal? (s/n)"
Test-Step "fechar o app tira você da chamada" ($resposta -eq "s")

if ($script:falhas -gt 0) { Write-Host "`n$($script:falhas) falha(s)." -ForegroundColor Red; exit 1 }
Write-Host "`nEtapa 4c OK." -ForegroundColor Green
