# Teste da etapa 4d: instalador do Windows e atualização automática.
#
# Uso:
#   powershell -ExecutionPolicy Bypass -File scripts\teste-instalador.ps1
#
# Precisa do cargo-packager (`cargo install cargo-packager --locked`). Gera o instalador,
# abre-o e pergunta o que você viu. O ideal é instalar também numa máquina (ou conta do
# Windows) que nunca rodou o projeto: o script mostra onde ficou o instalador.

$ErrorActionPreference = "Stop"
. "$PSScriptRoot\comum.ps1"

if (-not (Get-Command cargo-packager -ErrorAction SilentlyContinue)) {
    throw "falta o cargo-packager: cargo install cargo-packager --locked"
}
Push-Location "$PSScriptRoot\.."
try {
    Write-Host "Gerando o instalador (build de release; a primeira vez demora)..."
    cargo packager --release -p call-desktop
    if ($LASTEXITCODE -ne 0) { throw "o instalador não foi gerado" }
    $target = (cargo metadata -q --format-version 1 --no-deps | ConvertFrom-Json).target_directory
    $setup = Get-ChildItem "$target\release" -Filter "*-setup.exe" | Sort-Object LastWriteTime | Select-Object -Last 1
} finally {
    Pop-Location
}
Write-Host "Instalador: $($setup.FullName) ($([int]($setup.Length / 1MB)) MB)"
Test-Step "o instalador foi gerado" ($null -ne $setup)

Write-Host ""
Write-Host "1. O instalador vai abrir. Instale com as opções padrão (não pede administrador)." -ForegroundColor Cyan
Start-Process $setup.FullName -Wait

$itens = @(
    "o instalador e o atalho do Menu Iniciar mostram o ícone do Call (o 'C' com o ponto verde)",
    "o app abre pelo atalho, sem janela de console, com o ícone na barra de tarefas",
    "dá para entrar, abrir um servidor e falar no canal de voz",
    "o app aparece em 'Aplicativos instalados' do Windows e desinstala por lá"
)
foreach ($item in $itens) {
    $resposta = Read-Host "Confere: $($item)? (s/n)"
    Test-Step $item ($resposta -eq "s")
}

if ($script:falhas -gt 0) { Write-Host "`n$($script:falhas) falha(s)." -ForegroundColor Red; exit 1 }
Write-Host "`nEtapa 4d (instalador) OK." -ForegroundColor Green
