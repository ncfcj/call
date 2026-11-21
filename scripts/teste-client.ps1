# Parceiro de conversa para testar o client no navegador.
#
# Cria um usuário "robo", entra no seu servidor pelo código de convite e conversa pelo
# WebSocket: aparece online, digita, manda uma mensagem e responde cada mensagem sua com um eco.
# Assim dá para testar o chat em tempo real com um navegador só.
#
# Uso (com a API e o client rodando, e você logado no navegador):
#   powershell -ExecutionPolicy Bypass -File scripts\teste-client.ps1 -Convite <código>
#
# O código aparece na barra de canais, abaixo do nome do servidor. O robô fica 3 minutos
# (mude com -Minutos) e sai do servidor no fim; feche antes com Ctrl+C.

param(
    [Parameter(Mandatory = $true)][string]$Convite,
    [string]$Api = "http://127.0.0.1:3000",
    [int]$Minutos = 3
)

$ErrorActionPreference = "Stop"
. "$PSScriptRoot\comum.ps1"

$sufixo = Get-Random -Maximum 99999
$robo = New-User "robo$sufixo"
$r = Invoke-Api POST "/invites/$Convite" $null $robo
if ($r.Status -ne 200) { throw "Convite $Convite não encontrado (status $($r.Status))" }
$g = $r.Body.id
Write-Host "Robô entrou em '$($r.Body.name)'. Ele aparece online na lista de membros."

$canal = @((Invoke-Api GET "/guilds/$g/channels" $null $robo).Body | Where-Object { $_.kind -eq "text" })[0].id
$ws = Connect-Ws $robo
$meuId = (Receive-Ws $ws).d.user_id
Send-Ws $ws @{ t = "SUBSCRIBE"; d = @{ guild_id = $g } }
[void](Receive-WsOf $ws "SUBSCRIBED")

Start-Sleep -Seconds 2
Send-Ws $ws @{ t = "TYPING_START"; d = @{ channel_id = $canal } }
Write-Host "Robô digitando... (veja o aviso embaixo das mensagens)"
Start-Sleep -Seconds 3
[void](Invoke-Api POST "/channels/$canal/messages" @{ content = "Olá! Sou o robô de teste. Mande uma mensagem que eu respondo." } $robo)

$fim = (Get-Date).AddMinutes($Minutos)
$ultimoHeartbeat = Get-Date
Write-Host "Respondendo até $($fim.ToString('HH:mm:ss')). Ctrl+C encerra."
try {
    while ((Get-Date) -lt $fim) {
        if (((Get-Date) - $ultimoHeartbeat).TotalSeconds -ge 25) {
            Send-Ws $ws @{ t = "HEARTBEAT" }
            $ultimoHeartbeat = Get-Date
        }
        $event = Receive-Ws $ws 1000
        if ($null -eq $event) { continue }
        if ($event.t -eq "MESSAGE_CREATE" -and $event.d.author.id -ne $meuId) {
            $quem = $event.d.author.display_name
            Write-Host "  $quem disse: $($event.d.content)"
            Send-Ws $ws @{ t = "TYPING_START"; d = @{ channel_id = $event.d.channel_id } }
            Start-Sleep -Milliseconds 1500
            [void](Invoke-Api POST "/channels/$($event.d.channel_id)/messages" @{ content = "Eco para $($quem): $($event.d.content)" } $robo)
        }
    }
} finally {
    Close-Ws $ws
    [void](Invoke-Api DELETE "/guilds/$g/members/@me" $null $robo)
    Write-Host "Robô saiu do servidor."
}
