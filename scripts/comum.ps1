# Funções comuns dos testes funcionais. Use com: . "$PSScriptRoot\comum.ps1"
# Espera que o script que importa defina $Api.

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

# --- WebSocket -------------------------------------------------------------------------------
# ClientWebSocket do .NET. Um ReceiveAsync cancelado derruba o socket no .NET Framework, então
# a leitura com prazo espera a tarefa pendente sem cancelá-la e a guarda para a próxima leitura.

function Open-Ws([string]$Ticket) {
    $ws = New-Object System.Net.WebSockets.ClientWebSocket
    $uri = [Uri]("$($Api -replace '^http', 'ws')/ws?ticket=$Ticket")
    $ws.ConnectAsync($uri, [Threading.CancellationToken]::None).Wait()
    return @{ Socket = $ws; Pending = $null; Buffer = New-Object byte[] 65536 }
}

# Pede um ticket com o access token e conecta.
function Connect-Ws([string]$Token) {
    $ticket = (Invoke-Api POST "/ws/ticket" $null $Token).Body.ticket
    return Open-Ws $ticket
}

function Send-Ws($Client, $Event) {
    $bytes = [System.Text.Encoding]::UTF8.GetBytes(($Event | ConvertTo-Json -Compress -Depth 5))
    $segment = [System.ArraySegment[byte]]::new($bytes)
    $Client.Socket.SendAsync($segment, "Text", $true, [Threading.CancellationToken]::None).Wait()
}

# Próximo evento, ou $null se nada chegar no prazo ou a conexão fechar.
function Receive-Ws($Client, [int]$TimeoutMs = 3000) {
    $text = New-Object System.Text.StringBuilder
    while ($true) {
        if ($null -eq $Client.Pending) {
            $segment = [System.ArraySegment[byte]]::new($Client.Buffer)
            $Client.Pending = $Client.Socket.ReceiveAsync($segment, [Threading.CancellationToken]::None)
        }
        if (-not $Client.Pending.Wait($TimeoutMs)) { return $null }
        $result = $Client.Pending.Result
        $Client.Pending = $null
        if ($result.MessageType -eq "Close") { return $null }
        [void]$text.Append([System.Text.Encoding]::UTF8.GetString($Client.Buffer, 0, $result.Count))
        if ($result.EndOfMessage) { return $text.ToString() | ConvertFrom-Json }
    }
}

# Próximo evento do tipo $Type, pulando os outros.
function Receive-WsOf($Client, [string]$Type, [int]$TimeoutMs = 3000) {
    while ($true) {
        $event = Receive-Ws $Client $TimeoutMs
        if ($null -eq $event -or $event.t -eq $Type) { return $event }
    }
}

# Fecha do lado do client sem esperar a resposta.
function Close-Ws($Client) {
    $Client.Socket.CloseOutputAsync("NormalClosure", "tchau", [Threading.CancellationToken]::None).Wait()
}
