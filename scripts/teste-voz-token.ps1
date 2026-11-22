# Teste funcional da etapa "token de voz" da Fase 2.
#
# Pede tokens de voz à API e confere com o próprio LiveKit (endpoint /rtc/validate) que ele
# aceita o token. Também confere as recusas: canal de texto, quem está fora e token adulterado.
# Cada passo imprime OK ou FALHOU.
#
# Uso (com `docker compose up -d` e `cargo run -p call-server` em outro terminal):
#   powershell -ExecutionPolicy Bypass -File scripts\teste-voz-token.ps1

param([string]$Api = "http://127.0.0.1:3000")

$ErrorActionPreference = "Stop"
. "$PSScriptRoot\comum.ps1"

# GET simples que não lança exceção: devolve @{ Status; Text }.
function Get-Text([string]$Url) {
    $req = [System.Net.HttpWebRequest]::Create($Url)
    try { $res = $req.GetResponse() } catch [System.Net.WebException] {
        $res = $_.Exception.Response
        if ($null -eq $res) { return @{ Status = 0; Text = "sem resposta" } }
    }
    $reader = New-Object System.IO.StreamReader($res.GetResponseStream())
    $text = $reader.ReadToEnd()
    $reader.Close()
    $status = [int]$res.StatusCode
    $res.Close()
    return @{ Status = $status; Text = $text.Trim() }
}

$sufixo = Get-Random -Maximum 99999
Write-Host "API: $Api   (sufixo dos usuários: $sufixo)`n"

Write-Host "Preparação"
$ana = New-User "ana$sufixo"
$eve = New-User "eve$sufixo"
$guild = (Invoke-Api POST "/guilds" @{ name = "Sala de voz" } $ana).Body
$canais = @((Invoke-Api GET "/guilds/$($guild.id)/channels" $null $ana).Body)
$voz = ($canais | Where-Object { $_.kind -eq "voice" })[0].id
$texto = ($canais | Where-Object { $_.kind -eq "text" })[0].id
Test-Step "ana cria um servidor com o canal de voz Geral" ($null -ne $voz)

Write-Host "`nToken"
$r = Invoke-Api POST "/channels/$voz/voice/token" $null $ana
Test-Step "ana recebe um token para o canal de voz" ($r.Status -eq 200 -and $r.Body.token)
Test-Step "o token vale por 5 min" ($r.Body.expires_in -eq 300)
$livekit = $r.Body.url
Write-Host "          LiveKit em $livekit"

Write-Host "`nLiveKit"
$http = $livekit -replace '^ws', 'http'
$v = Get-Text "$http/rtc/validate?access_token=$($r.Body.token)"
Test-Step "o LiveKit aceita o token" ($v.Status -eq 200) "(status $($v.Status): $($v.Text))"
# Troca o primeiro caractere da assinatura. O último não serve: em base64url ele carrega só
# 4 bits, e trocar "w" por "x", por exemplo, pode dar a mesma assinatura.
$partes = $r.Body.token.Split(".")
$troca = if ($partes[2][0] -eq "A") { "B" } else { "A" }
$partes[2] = $troca + $partes[2].Substring(1)
$adulterado = $partes -join "."
$v = Get-Text "$http/rtc/validate?access_token=$adulterado"
Test-Step "o LiveKit recusa um token adulterado" ($v.Status -eq 401) "(status $($v.Status))"

Write-Host "`nRecusas da API"
$r = Invoke-Api POST "/channels/$texto/voice/token" $null $ana
Test-Step "canal de texto não tem chamada (422)" ($r.Status -eq 422)
$r = Invoke-Api POST "/channels/$voz/voice/token" $null $eve
Test-Step "eve, de fora, não recebe token (404)" ($r.Status -eq 404)
$r = Invoke-Api POST "/channels/$voz/voice/token"
Test-Step "sem login não recebe token (401)" ($r.Status -eq 401)

Write-Host ""
if ($script:falhas -eq 0) {
    Write-Host "Tudo certo." -ForegroundColor Green
} else {
    Write-Host "$($script:falhas) passo(s) falharam." -ForegroundColor Red
    exit 1
}
