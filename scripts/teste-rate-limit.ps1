# Teste funcional da etapa 3a (API pronta para a internet).
#
# Confere o limite de tentativas (429 com Retry-After), o tamanho máximo do corpo (413) e o
# formato dos erros. Cada passo imprime OK ou FALHOU.
# Deixa o login deste computador no limite: espere 1 minuto para entrar de novo pelo client.
#
# Uso (com `docker compose up -d` e `cargo run -p call-server` em outro terminal):
#   powershell -ExecutionPolicy Bypass -File scripts\teste-rate-limit.ps1

param([string]$Api = "http://127.0.0.1:3000")

$ErrorActionPreference = "Stop"
. "$PSScriptRoot\comum.ps1"

# Como o Invoke-Api, mas com o corpo cru e o header Retry-After na resposta.
function Invoke-Raw([string]$Method, [string]$Path, [string]$Text = "", [string]$Token = "") {
    $req = [System.Net.HttpWebRequest]::Create("$Api$Path")
    $req.Method = $Method
    if ($Token) { $req.Headers.Add("Authorization", "Bearer $Token") }
    if ($Method -ne "GET") {
        $bytes = [System.Text.Encoding]::UTF8.GetBytes($Text)
        $req.ContentType = "application/json"
        $stream = $req.GetRequestStream()
        $stream.Write($bytes, 0, $bytes.Length)
        $stream.Close()
    }
    try { $res = $req.GetResponse() } catch [System.Net.WebException] {
        $res = $_.Exception.Response
        if ($null -eq $res) { throw "sem resposta de $Api" }
    }
    $reader = New-Object System.IO.StreamReader($res.GetResponseStream(), [System.Text.Encoding]::UTF8)
    $body = $null
    try { $body = $reader.ReadToEnd() | ConvertFrom-Json } catch { }
    $reader.Close()
    $r = @{ Status = [int]$res.StatusCode; Body = $body; RetryAfter = $res.Headers["Retry-After"] }
    $res.Close()
    return $r
}

Write-Host "API: $Api`n"

Write-Host "Formato dos erros"
$ana = New-User "ana$(Get-Random -Maximum 99999)"
$r = Invoke-Raw POST "/guilds" "{" $ana
Test-Step "JSON malformado (400, validation)" ($r.Status -eq 400 -and $r.Body.code -eq "validation") "(status $($r.Status))"
$r = Invoke-Raw GET "/guilds/nao-e-uuid" "" $ana
Test-Step "UUID inválido na URL (400, validation)" ($r.Status -eq 400 -and $r.Body.code -eq "validation") "(status $($r.Status))"
$r = Invoke-Raw GET "/nada"
Test-Step "rota inexistente (404, not_found)" ($r.Status -eq 404 -and $r.Body.code -eq "not_found") "(status $($r.Status))"

Write-Host "`nTamanho do corpo"
$r = Invoke-Raw POST "/guilds" ('{"name":"' + ("x" * 70000) + '"}') $ana
Test-Step "corpo de 70 KB é recusado (413)" ($r.Status -eq 413 -and $r.Body.code -eq "validation") "(status $($r.Status))"

Write-Host "`nLimite de mensagens (por usuário)"
$guild = (Invoke-Api POST "/guilds" @{ name = "Limites" } $ana).Body
$canais = @((Invoke-Api GET "/guilds/$($guild.id)/channels" $null $ana).Body)
$texto = ($canais | Where-Object { $_.kind -eq "text" })[0].id
$status = 1..20 | ForEach-Object { (Invoke-Api POST "/channels/$texto/messages" @{ content = "oi $_" } $ana).Status }
Test-Step "as 10 primeiras passam" (@($status[0..9] | Where-Object { $_ -ne 201 }).Count -eq 0) "($status)"
Test-Step "a rajada é barrada (429)" ($status -contains 429) "($status)"
Start-Sleep -Seconds 2
$r = Invoke-Api POST "/channels/$texto/messages" @{ content = "de volta" } $ana
Test-Step "depois de esperar, volta a enviar" ($r.Status -eq 201) "(status $($r.Status))"

Write-Host "`nLimite de login (por IP)"
$login = '{"username":"ninguem","password":"errada"}'
$respostas = 1..15 | ForEach-Object { Invoke-Raw POST "/auth/login" $login }
$barrado = $respostas | Where-Object { $_.Status -eq 429 } | Select-Object -First 1
Test-Step "logins seguidos são barrados (429)" ($null -ne $barrado)
Test-Step "com o código rate_limited" ($barrado.Body.code -eq "rate_limited")
Test-Step "e o Retry-After" ([int]$barrado.RetryAfter -ge 1) "($($barrado.RetryAfter))"
Write-Host "          mensagem: $($barrado.Body.message)"

Write-Host ""
if ($script:falhas -eq 0) {
    Write-Host "Tudo certo." -ForegroundColor Green
} else {
    Write-Host "$($script:falhas) passo(s) falharam." -ForegroundColor Red
    exit 1
}
