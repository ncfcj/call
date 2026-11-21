# Teste funcional da etapa "WebSocket" da Fase 1.
#
# Abre três conexões WebSocket ao mesmo tempo (ana e bia, membros, e eve, de fora) e confere
# que mensagem, digitação e presença chegam em tempo real só a quem é membro do servidor.
# Cada passo imprime OK ou FALHOU.
#
# Uso (com `docker compose up -d` e `cargo run -p call-server` em outro terminal):
#   powershell -ExecutionPolicy Bypass -File scripts\teste-websocket.ps1

param([string]$Api = "http://127.0.0.1:3000")

$ErrorActionPreference = "Stop"
. "$PSScriptRoot\comum.ps1"

$sufixo = Get-Random -Maximum 99999
Write-Host "API: $Api   (sufixo dos usuários: $sufixo)`n"

Write-Host "Preparação"
$ana = New-User "ana$sufixo"
$bia = New-User "bia$sufixo"
$eve = New-User "eve$sufixo"
$guild = (Invoke-Api POST "/guilds" @{ name = "Sala da Ana" } $ana).Body
$g = $guild.id
$r = Invoke-Api POST "/invites/$($guild.invite_code)" $null $bia
Test-Step "ana cria o servidor e bia entra pelo convite" ($r.Status -eq 200)
$canal = @((Invoke-Api GET "/guilds/$g/channels" $null $ana).Body)[0].id

Write-Host "`nTicket"
$r = Invoke-Api POST "/ws/ticket"
Test-Step "pedir ticket sem login dá 401" ($r.Status -eq 401)
$ticket = (Invoke-Api POST "/ws/ticket" $null $ana).Body.ticket
$wsAna = Open-Ws $ticket
$hello = Receive-Ws $wsAna
Test-Step "ana conecta e recebe HELLO com heartbeat de 30 s" (
    $hello.t -eq "HELLO" -and $hello.d.heartbeat_interval_ms -eq 30000)
$anaId = $hello.d.user_id
$reuso = $false
try { Open-Ws $ticket | Out-Null } catch { $reuso = $true }
Test-Step "o mesmo ticket não conecta de novo" $reuso
$inventado = $false
try { Open-Ws "inventado" | Out-Null } catch { $inventado = $true }
Test-Step "ticket inventado não conecta" $inventado

Write-Host "`nHeartbeat"
Send-Ws $wsAna @{ t = "HEARTBEAT" }
Test-Step "HEARTBEAT recebe HEARTBEAT_ACK" ((Receive-Ws $wsAna).t -eq "HEARTBEAT_ACK")

Write-Host "`nInscrição e presença"
Send-Ws $wsAna @{ t = "SUBSCRIBE"; d = @{ guild_id = $g } }
$sub = Receive-WsOf $wsAna "SUBSCRIBED"
Test-Step "ana se inscreve e é a única online" (@($sub.d.online).Count -eq 1 -and $sub.d.online[0] -eq $anaId)

$wsBia = Connect-Ws $bia
$biaId = (Receive-Ws $wsBia).d.user_id
Send-Ws $wsBia @{ t = "SUBSCRIBE"; d = @{ guild_id = $g } }
$sub = Receive-WsOf $wsBia "SUBSCRIBED"
Test-Step "bia se inscreve e vê 2 online" (@($sub.d.online).Count -eq 2)
$p = Receive-WsOf $wsAna "PRESENCE_UPDATE"
Test-Step "ana recebe PRESENCE_UPDATE: bia online" ($p.d.user_id -eq $biaId -and $p.d.status -eq "online")

$wsEve = Connect-Ws $eve
[void](Receive-Ws $wsEve)
Send-Ws $wsEve @{ t = "SUBSCRIBE"; d = @{ guild_id = $g } }
$err = Receive-Ws $wsEve
Test-Step "eve, de fora, não se inscreve (ERROR not_found)" ($err.t -eq "ERROR" -and $err.d.code -eq "not_found")

Write-Host "`nDigitação"
Send-Ws $wsBia @{ t = "TYPING_START"; d = @{ channel_id = $canal } }
$typing = Receive-WsOf $wsAna "TYPING_START"
Test-Step "ana vê que bia está digitando" ($typing.d.user_id -eq $biaId -and $typing.d.channel_id -eq $canal)

Write-Host "`nMensagem em tempo real"
$texto = "oi bia, chegou na hora? ação ✓"
$enviada = (Invoke-Api POST "/channels/$canal/messages" @{ content = $texto } $ana).Body
$recebida = Receive-WsOf $wsBia "MESSAGE_CREATE"
Test-Step "bia recebe o MESSAGE_CREATE com acentos" ($recebida.d.id -eq $enviada.id -and $recebida.d.content -eq $texto) "($($recebida.d.content))"
$eco = Receive-WsOf $wsAna "MESSAGE_CREATE"
Test-Step "ana também recebe (outras abas dela ficam em dia)" ($eco.d.id -eq $enviada.id)
Test-Step "eve não recebe nada" ($null -eq (Receive-Ws $wsEve 500))

Write-Host "`nSaída"
Close-Ws $wsBia
$p = Receive-WsOf $wsAna "PRESENCE_UPDATE"
Test-Step "bia fecha a conexão e ana a vê offline" ($p.d.user_id -eq $biaId -and $p.d.status -eq "offline")

Close-Ws $wsAna
Close-Ws $wsEve

Write-Host ""
if ($script:falhas -eq 0) {
    Write-Host "Tudo certo." -ForegroundColor Green
    Write-Host "Login para inspecionar: ana$sufixo / senha-forte-1, canal $canal"
} else {
    Write-Host "$($script:falhas) passo(s) falharam." -ForegroundColor Red
    exit 1
}
