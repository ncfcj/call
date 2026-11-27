# Teste da etapa 4f: Push-to-Talk no app desktop, numa chamada com a web.
#
# Uso (por padrão contra a produção, que tem o LiveKit):
#   powershell -ExecutionPolicy Bypass -File scripts\teste-desktop-ptt.ps1
#
# Precisa de alguém (ou de você mesmo, no celular) na web, no mesmo canal de voz, para ouvir
# quando o seu microfone abre.

param([string]$Api = "https://call-70hr.onrender.com")

$ErrorActionPreference = "Stop"
. "$PSScriptRoot\comum.ps1"

Write-Host "Acordando a API (o Render free pode levar um minuto)..."
$health = Invoke-Api GET "/health"
if ($health.Status -ne 200) { throw "API fora do ar em $Api" }
Write-Host "Compilando o app..."
cargo build -q -p call-desktop
if ($LASTEXITCODE -ne 0) { throw "o app não compilou" }
$env:CALL_API_URL = $Api
$exe = Join-Path (cargo metadata -q --format-version 1 --no-deps | ConvertFrom-Json).target_directory "debug\call-desktop.exe"
Start-Process $exe | Out-Null
Write-Host "O app abriu na janela 'Call' (veja na barra de tarefas se ficou atrás desta)."

Write-Host ""
Write-Host "1. No app e na web (outra conta), entrem no mesmo canal de voz." -ForegroundColor Cyan
Write-Host "2. No app, clique em 'Áudio' e, em 'Apertar para falar', em 'Escolher tecla'."
Write-Host "   Aperte F8 (ou outra tecla que você não usa). Deve aparecer 'Tecla: F8'. Feche o diálogo."
Read-Host "Enter quando estiver na chamada com a tecla escolhida"

$itens = @(
    "sem apertar a tecla, a web não ouve você e o seu nome no app mostra '(mudo)'",
    "segurando a tecla, a web ouve você; soltando, para em menos de meio segundo",
    "com outro programa na frente (o navegador, o Bloco de Notas), segurar a tecla também abre o microfone",
    "com 'Mudo' ligado, segurar a tecla não abre o microfone",
    "em 'Áudio', 'Desligar' volta ao microfone sempre aberto (a web ouve sem apertar nada)",
    "em 'Escolher tecla', Esc cancela sem mudar nada"
)
foreach ($item in $itens) {
    $resposta = Read-Host "Confere: $($item)? (s/n)"
    Test-Step $item ($resposta -eq "s")
}

if ($script:falhas -gt 0) { Write-Host "`n$($script:falhas) falha(s)." -ForegroundColor Red; exit 1 }
Write-Host "`nEtapa 4f OK." -ForegroundColor Green
