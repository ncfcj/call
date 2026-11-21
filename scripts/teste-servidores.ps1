# Teste funcional da etapa "servidores, canais e mensagens" da Fase 1.
#
# Passa pelo fluxo real contra o servidor rodando: cadastro, criação de servidor, convite,
# permissões de dono, envio e histórico de mensagens. Cada passo imprime OK ou FALHOU.
#
# Uso (com `docker compose up -d` e `cargo run -p call-server` em outro terminal):
#   powershell -ExecutionPolicy Bypass -File scripts\teste-servidores.ps1
#   powershell -ExecutionPolicy Bypass -File scripts\teste-servidores.ps1 -Api http://127.0.0.1:3000
#
# Os usuários e o servidor criados ficam no banco, para você inspecionar depois.

param([string]$Api = "http://127.0.0.1:3000")

$ErrorActionPreference = "Stop"
$script:falhas = 0

# Chama a API e devolve @{ Status; Body }. Não lança exceção em 4xx/5xx.
# O corpo vai e volta em UTF-8 (o PowerShell 5.1 erra os acentos se deixar por conta dele).
function Invoke-Api([string]$Method, [string]$Path, $Body = $null, [string]$Token = $null) {
    $req = [System.Net.HttpWebRequest]::Create("$Api$Path")
    $req.Method = $Method
    if ($Token) { $req.Headers.Add("Authorization", "Bearer $Token") }
    if ($null -ne $Body) {
        $bytes = [System.Text.Encoding]::UTF8.GetBytes(($Body | ConvertTo-Json -Compress -Depth 5))
        $req.ContentType = "application/json; charset=utf-8"
        $stream = $req.GetRequestStream()
        $stream.Write($bytes, 0, $bytes.Length)
        $stream.Close()
    }
    try { $res = $req.GetResponse() } catch [System.Net.WebException] {
        $res = $_.Exception.Response
        if ($null -eq $res) { throw "API fora do ar em $Api. Rode: cargo run -p call-server" }
    }
    $reader = New-Object System.IO.StreamReader($res.GetResponseStream(), [System.Text.Encoding]::UTF8)
    $text = $reader.ReadToEnd()
    $reader.Close()
    $status = [int]$res.StatusCode
    $res.Close()
    $parsed = $null
    if ($text) { try { $parsed = $text | ConvertFrom-Json } catch { $parsed = $text } }
    return @{ Status = $status; Body = $parsed }
}

function Test-Step([string]$Name, [bool]$Ok, [string]$Detail = "") {
    if ($Ok) {
        Write-Host "  OK      $Name" -ForegroundColor Green
    } else {
        $script:falhas++
        Write-Host "  FALHOU  $Name $Detail" -ForegroundColor Red
    }
}

function New-User([string]$Name) {
    $r = Invoke-Api POST "/auth/register" @{ username = $Name; display_name = $Name; password = "senha-forte-1" }
    Test-Step "cadastra $Name" ($r.Status -eq 201) "(status $($r.Status))"
    return $r.Body.access_token
}

$sufixo = Get-Random -Maximum 99999
Write-Host "API: $Api   (sufixo dos usuários: $sufixo)`n"

Write-Host "Saúde"
$r = Invoke-Api GET "/health"
Test-Step "GET /health responde" ($r.Status -eq 200)

Write-Host "`nUsuários"
$ana = New-User "ana$sufixo"
$bia = New-User "bia$sufixo"
$eve = New-User "eve$sufixo"

Write-Host "`nServidor"
$r = Invoke-Api POST "/guilds" @{ name = "Amigos da Ana" } $ana
Test-Step "ana cria o servidor" ($r.Status -eq 201)
$guild = $r.Body
$g = $guild.id
Write-Host "          id $g, convite $($guild.invite_code)"

$r = Invoke-Api GET "/guilds/$g/channels" $null $ana
$canais = @($r.Body)
Test-Step "servidor nasce com os canais geral (texto) e Geral (voz)" (
    $canais.Count -eq 2 -and $canais[0].kind -eq "text" -and $canais[1].kind -eq "voice")
$texto = $canais[0].id
$voz = $canais[1].id

Write-Host "`nConvite"
$r = Invoke-Api GET "/guilds/$g" $null $bia
Test-Step "bia ainda não vê o servidor (404)" ($r.Status -eq 404)
$r = Invoke-Api POST "/invites/$($guild.invite_code)" $null $bia
Test-Step "bia entra pelo convite" ($r.Status -eq 200 -and $r.Body.id -eq $g)
$r = Invoke-Api POST "/invites/$($guild.invite_code)" $null $bia
Test-Step "entrar de novo não dá erro" ($r.Status -eq 200)
$r = Invoke-Api GET "/guilds/$g/members" $null $bia
Test-Step "o servidor tem 2 membros" (@($r.Body).Count -eq 2)

Write-Host "`nPermissões de dono"
$r = Invoke-Api PATCH "/guilds/$g" @{ name = "Tomado" } $bia
Test-Step "bia não renomeia o servidor (403)" ($r.Status -eq 403)
$r = Invoke-Api POST "/guilds/$g/channels" @{ name = "jogos"; kind = "text" } $bia
Test-Step "bia não cria canal (403)" ($r.Status -eq 403)
$r = Invoke-Api POST "/guilds/$g/channels" @{ name = "jogos"; kind = "text" } $ana
Test-Step "ana cria o canal jogos no fim da lista" ($r.Status -eq 201 -and $r.Body.position -eq 2)
$r = Invoke-Api DELETE "/guilds/$g/members/@me" $null $ana
Test-Step "o dono não sai do servidor (403)" ($r.Status -eq 403)

Write-Host "`nMensagens"
$conteudos = @("olá, bia!", "tudo bem?", "oi ana, tudo ótimo", "vamos jogar?", "bora")
$autores = @($ana, $ana, $bia, $ana, $bia)
$ids = @()
for ($i = 0; $i -lt $conteudos.Count; $i++) {
    $r = Invoke-Api POST "/channels/$texto/messages" @{ content = $conteudos[$i] } $autores[$i]
    $ids += $r.Body.id
}
Test-Step "ana e bia enviam 5 mensagens" ($ids.Count -eq 5 -and $null -notin $ids)

$r = Invoke-Api GET "/channels/$texto/messages" $null $bia
$lidas = @($r.Body | ForEach-Object { $_.content })
Test-Step "histórico em ordem cronológica, com acentos" (($lidas -join "|") -eq ($conteudos -join "|")) "($($lidas -join ' | '))"

$r = Invoke-Api GET "/channels/$texto/messages?limit=2&before=$($ids[3])" $null $bia
$pagina = @($r.Body | ForEach-Object { $_.content })
Test-Step "before: as 2 anteriores à 4a mensagem" (($pagina -join "|") -eq "tudo bem?|oi ana, tudo ótimo")

$r = Invoke-Api GET "/channels/$texto/messages?after=$($ids[2])" $null $bia
$pagina = @($r.Body | ForEach-Object { $_.content })
Test-Step "after: o que veio depois da 3a (reconexão)" (($pagina -join "|") -eq "vamos jogar?|bora")

$r = Invoke-Api POST "/channels/$texto/messages" @{ content = "   " } $ana
Test-Step "mensagem só de espaços é recusada (422)" ($r.Status -eq 422)
$r = Invoke-Api GET "/channels/$voz/messages" $null $ana
Test-Step "canal de voz não tem mensagens (422)" ($r.Status -eq 422)

Write-Host "`nQuem está fora"
$r = Invoke-Api GET "/channels/$texto/messages" $null $eve
Test-Step "eve não lê o histórico (404)" ($r.Status -eq 404)
$r = Invoke-Api POST "/channels/$texto/messages" @{ content = "spam" } $eve
Test-Step "eve não envia mensagem (404)" ($r.Status -eq 404)
$r = Invoke-Api GET "/users/@me/guilds" $null $eve
Test-Step "eve não tem servidores" (@($r.Body).Count -eq 0)

Write-Host ""
if ($script:falhas -eq 0) {
    Write-Host "Tudo certo." -ForegroundColor Green
    Write-Host "Login para inspecionar: ana$sufixo / senha-forte-1, servidor $g"
} else {
    Write-Host "$($script:falhas) passo(s) falharam." -ForegroundColor Red
    exit 1
}
