# Etapa 4e: mede CPU, GPU e memória do app desktop ou do Chrome por alguns segundos.
#
# Uso (deixe o cenário montado antes e não mexa no PC enquanto mede):
#   powershell -ExecutionPolicy Bypass -File scripts\medir.ps1 -Alvo desktop -Cenario "parado"
#   powershell -ExecutionPolicy Bypass -File scripts\medir.ps1 -Alvo desktop -Cenario "chamada"
#   powershell -ExecutionPolicy Bypass -File scripts\medir.ps1 -Alvo chrome  -Cenario "parado"
#   powershell -ExecutionPolicy Bypass -File scripts\medir.ps1 -Alvo chrome  -Cenario "chamada"
#
# Memória é o "conjunto de trabalho privado", a coluna Memória do Gerenciador de Tarefas. CPU é a
# fatia de todos os núcleos. No Chrome, soma todos os processos dele: deixe aberta só a aba do Call.

param(
    [ValidateSet("desktop", "chrome")][string]$Alvo = "desktop",
    [string]$Cenario = "",
    [int]$Segundos = 60
)

$ErrorActionPreference = "Stop"
$nome = if ($Alvo -eq "desktop") { "call-desktop" } else { "chrome" }

function Get-Alvo { @(Get-Process -Name $nome -ErrorAction SilentlyContinue) }

if ((Get-Alvo).Count -eq 0) { throw "nenhum processo '$nome' aberto" }
$nucleos = [Environment]::ProcessorCount
Write-Host "Medindo $Alvo ($Cenario) por $Segundos s..."

# Tempo de CPU de cada processo, para tirar a diferença a cada segundo.
$antes = @{}
Get-Alvo | ForEach-Object { $antes[$_.Id] = $_.TotalProcessorTime.TotalMilliseconds }
$cpu = @(); $mem = @(); $gpu = @()
for ($i = 0; $i -lt $Segundos; $i++) {
    Start-Sleep 1
    $procs = Get-Alvo
    $ms = 0
    foreach ($p in $procs) {
        $agora = $p.TotalProcessorTime.TotalMilliseconds
        if ($antes.ContainsKey($p.Id)) { $ms += $agora - $antes[$p.Id] }
        $antes[$p.Id] = $agora
    }
    $cpu += $ms / 10 / $nucleos
    $ids = $procs.Id
    $mem += ((Get-CimInstance Win32_PerfFormattedData_PerfProc_Process |
        Where-Object { $ids -contains $_.IDProcess } | Measure-Object WorkingSetPrivate -Sum).Sum) / 1MB
    # GPU: o motor 3D mais usado, como o Gerenciador mostra.
    $motores = Get-CimInstance Win32_PerfFormattedData_GPUPerformanceCounters_GPUEngine -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -match '^pid_(\d+)_' -and $ids -contains [int]$Matches[1] -and $_.Name -match 'engtype_3D' }
    $gpu += ($motores | Measure-Object UtilizationPercentage -Sum).Sum
}

function Get-Media($v) { ($v | Measure-Object -Average).Average }
function Get-Maximo($v) { ($v | Measure-Object -Maximum).Maximum }
Write-Host ""
"{0} | {1} | CPU média {2:N2}% (máx {3:N1}%) | memória média {4:N0} MB (máx {5:N0}) | GPU média {6:N1}% (máx {7:N1}%)" -f `
    $Alvo, $Cenario, (Get-Media $cpu), (Get-Maximo $cpu), (Get-Media $mem), (Get-Maximo $mem), (Get-Media $gpu), (Get-Maximo $gpu)
