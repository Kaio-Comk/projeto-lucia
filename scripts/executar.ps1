# Projeto Lucia - executa o jogo e grava TUDO de uma execucao numa pasta so:
# o log do jogo, como o PC se comportou (CPU, RAM, commit, paginacao, disco,
# GPU, VRAM, temperatura, clock, consumo) e um resumo com o que importa.
#
# Uso (PowerShell, na pasta lucia):
#   .\scripts\executar.ps1 -Nome menus -Segundos 300 -Diag
#   .\scripts\executar.ps1 -Nome mundo -Segundos 900 -Diag -Teclas "10-120/5:Enter" -Captura
#   .\scripts\executar.ps1 -Nome teste -Env @{ VICTORY_TEXTURE_BUDGET_MB = "1536" }
#
# -Teclas: lista do VICTORY_KEYS ("30:Enter,35:X") ou intervalo "ini-fim/passo:Tecla".
#          Teclas so funcionam no build com -Diag (feature diagnostics).
# Saida: ..\runs\AAAA-MM-DD-HHMM-<nome>\ (fora do git; pode conter dados do jogo).
# Arquivos: config.txt, jogo.log, pc.csv, gpu.csv, bench.json, resumo.md, capturas\.

param(
    [string]$Nome = "execucao",
    [int]$Segundos = 300,
    [switch]$Diag,
    [string]$Teclas = "",
    [switch]$Captura,
    [hashtable]$Env = @{},
    [string]$Console = "series-s",
    [string]$Jogo = "..\GTA6\game_scarlett_master.exe"
)

$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent $PSScriptRoot
Set-Location $repo
$stamp = Get-Date -Format "yyyy-MM-dd-HHmm"
$dir = Join-Path (Split-Path -Parent $repo) "runs\$stamp-$Nome"
New-Item -ItemType Directory -Force $dir | Out-Null

# ---------- executavel e ambiente ----------
$exe = if ($Diag) { "target\diag\release\victory.exe" } else { "target\release\victory.exe" }
if (-not (Test-Path $exe)) { throw "Nao achei $exe. Compile antes (cargo build --release -p victory$(if($Diag){' --features diagnostics --target-dir target\diag'}))." }
if (-not (Test-Path (Join-Path (Split-Path $exe) "D3D12\D3D12Core.dll"))) { Write-Warning "Sem D3D12\D3D12Core.dll ao lado do exe: o Agility SDK nao vai carregar (SM 6.5, shaders recusados)." }

$padrao = @{
    VICTORY_DISPLAY_MODE      = "1280x720"
    VICTORY_TEXTURE_BUDGET_MB = "2048"
    VICTORY_CARD_BUFFERS_MB   = "512"
    VICTORY_D3D12_DRED        = "faults"
}
foreach ($k in $padrao.Keys) { if (-not $Env.ContainsKey($k)) { $Env[$k] = $padrao[$k] } }

if ($Teclas -match '^(\d+)-(\d+)/(\d+):(\w+)$') {
    $ini = [int]$Matches[1]; $fim = [int]$Matches[2]; $passo = [int]$Matches[3]; $tecla = $Matches[4]
    $Teclas = (($ini..$fim | Where-Object { ($_ - $ini) % $passo -eq 0 }) | ForEach-Object { "$($_):$tecla" }) -join ","
}
if ($Teclas) {
    if (-not $Diag) { Write-Warning "-Teclas sem -Diag: o build normal ignora VICTORY_KEYS." }
    $Env["VICTORY_KEYS"] = $Teclas
}
if ($Captura) {
    $cap = Join-Path $dir "capturas"; New-Item -ItemType Directory -Force $cap | Out-Null
    $Env["VICTORY_CAPTURE"] = $cap
    if (-not $Env.ContainsKey("VICTORY_CAPTURE_FRAMES")) { $Env["VICTORY_CAPTURE_FRAMES"] = ((1..60) | ForEach-Object { $_ * 300 }) -join "," }
}
foreach ($k in $Env.Keys) { Set-Item -Path "Env:$k" -Value $Env[$k] }

# ---------- config.txt: o que foi rodado e em que maquina ----------
$commit = (git rev-parse --short HEAD) 2>$null
$sujo = if (git status --porcelain --untracked-files=no) { " (com mudancas nao commitadas)" } else { "" }
$cpu = Get-CimInstance Win32_Processor | Select-Object -First 1
$os = Get-CimInstance Win32_OperatingSystem
$pf = Get-CimInstance Win32_PageFileUsage
$gpuInfo = (nvidia-smi --query-gpu=name,driver_version,memory.total --format=csv,noheader) 2>$null
$cfg = @(
    "inicio: $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')"
    "commit: $commit$sujo  branch: $((git branch --show-current) 2>$null)"
    "exe: $exe   diag: $([bool]$Diag)   console: $Console   duracao maxima: $Segundos s"
    "jogo: $Jogo"
    "cpu: $($cpu.Name.Trim()) ($($cpu.NumberOfCores)c/$($cpu.NumberOfLogicalProcessors)t)"
    "ram: $([int]($os.TotalVisibleMemorySize/1024)) MB  livre no inicio: $([int]($os.FreePhysicalMemory/1024)) MB"
    "pagefile: $(($pf | ForEach-Object { "$($_.Name) $($_.AllocatedBaseSize) MB" }) -join '; ')"
    "gpu: $gpuInfo"
    "windows: $($os.Caption) $($os.Version)"
    ""
    "variaveis:"
) + ($Env.Keys | Sort-Object | ForEach-Object { "  $_=$($Env[$_])" })
$cfg | Set-Content -Encoding UTF8 (Join-Path $dir "config.txt")

# ---------- coleta da GPU (nvidia-smi, 1 linha por segundo) ----------
$gpuCsv = Join-Path $dir "gpu.csv"
$smi = $null
if (Get-Command nvidia-smi -ErrorAction SilentlyContinue) {
    $smi = Start-Process -FilePath "nvidia-smi" -ArgumentList "--query-gpu=timestamp,temperature.gpu,utilization.gpu,utilization.memory,memory.used,clocks.gr,clocks.mem,power.draw,pstate,clocks_throttle_reasons.active --format=csv -lms 1000" -RedirectStandardOutput $gpuCsv -WindowStyle Hidden -PassThru
}

# ---------- o jogo ----------
$log = Join-Path $dir "jogo.log"
Write-Host "Executando ($Segundos s no maximo). Pasta: $dir"
$p = Start-Process -FilePath $exe -ArgumentList @("run", "`"$Jogo`"", "--execute", "--console", $Console) -RedirectStandardError $log -RedirectStandardOutput (Join-Path $dir "jogo.out") -PassThru

# ---------- coleta do PC (a cada 2 s) ----------
$pcCsv = Join-Path $dir "pc.csv"
"segundo,cpu_total_pct,cpu_processo_pct,ram_livre_mb,commit_mb,commit_limite_mb,paginas_lidas_s,disco_leitura_mb_s,processo_privado_mb,processo_ws_mb,threads" | Set-Content -Encoding UTF8 $pcCsv
$sw = [Diagnostics.Stopwatch]::StartNew()
$cores = [Environment]::ProcessorCount
$ultimoCpu = [TimeSpan]::Zero; $ultimoT = 0.0
while (-not $p.HasExited -and $sw.Elapsed.TotalSeconds -lt $Segundos) {
    Start-Sleep -Seconds 2
    try {
        $p.Refresh()
        $t = $sw.Elapsed.TotalSeconds
        $cpuProc = $p.TotalProcessorTime
        $pct = if ($t -gt $ultimoT) { [math]::Round(100 * ($cpuProc - $ultimoCpu).TotalSeconds / (($t - $ultimoT) * $cores), 1) } else { 0 }
        $ultimoCpu = $cpuProc; $ultimoT = $t
        $a = Get-CimInstance Win32_PerfFormattedData_PerfOS_Processor -Filter "Name='_Total'"
        $m = Get-CimInstance Win32_PerfFormattedData_PerfOS_Memory
        $d = Get-CimInstance Win32_PerfFormattedData_PerfDisk_PhysicalDisk -Filter "Name='_Total'"
        [string]::Format([Globalization.CultureInfo]::InvariantCulture, "{0},{1},{2},{3},{4},{5},{6},{7},{8},{9},{10}", [int]$t, $a.PercentProcessorTime, $pct, $m.AvailableMBytes, [int]($m.CommittedBytes / 1MB), [int]($m.CommitLimit / 1MB), $m.PagesInputPersec, [math]::Round($d.DiskReadBytesPersec / 1MB, 1), [int]($p.PrivateMemorySize64 / 1MB), [int]($p.WorkingSet64 / 1MB), $p.Threads.Count) | Add-Content $pcCsv
    } catch {}
}
$saiuSozinho = $p.HasExited
if (-not $saiuSozinho) { Stop-Process -Id $p.Id -Force; Start-Sleep -Seconds 1 }
$codigo = if ($saiuSozinho) { '0x{0:X8}' -f $p.ExitCode } else { "encerrado pelo script" }
if ($smi -and -not $smi.HasExited) { Stop-Process -Id $smi.Id -Force }
$duracao = [int]$sw.Elapsed.TotalSeconds

# ---------- bench.json (xtask bench da Trilha B) ----------
$bench = Join-Path $dir "bench.json"
cmd /c "cargo xtask bench `"$log`" --out `"$bench`" --label `"$Nome`" > `"$dir\bench.txt`" 2>&1"

# ---------- resumo.md ----------
$pc = Import-Csv $pcCsv
# nvidia-smi: "timestamp, temp, util %, memutil %, mem MiB, clock gr, clock mem, power W, pstate, reasons"
$gpu = @()
if (Test-Path $gpuCsv) {
    $gpu = Get-Content $gpuCsv | Select-Object -Skip 1 | Where-Object { $_ -match ',' } | ForEach-Object {
        $c = $_ -split ',\s*'
        [pscustomobject]@{ temp = $c[1]; util = $c[2]; mem = $c[4]; clock = $c[5]; power = $c[7]; reasons = $c[9] }
    }
}
function Num($v) { $x = 0.0; [void][double]::TryParse(("$v" -replace '[^0-9.]', ''), [Globalization.NumberStyles]::Float, [Globalization.CultureInfo]::InvariantCulture, [ref]$x); $x }
function Pico($linhas, $campo) { ($linhas | ForEach-Object { Num $_.$campo } | Measure-Object -Maximum).Maximum }
function Minimo($linhas, $campo) { ($linhas | ForEach-Object { Num $_.$campo } | Measure-Object -Minimum).Minimum }
function Media($linhas, $campo) { [math]::Round((($linhas | ForEach-Object { Num $_.$campo } | Measure-Object -Average).Average), 1) }

$texto = Get-Content $log -ErrorAction SilentlyContinue
$telas = @(); $ultimo = -1
foreach ($l in $texto) {
    if ($l -match '^\S+T(\d\d:\d\d:\d\d)\.\S+\s+INFO victory_gpu::host: ([0-9.]+) fps, ([0-9.]+) ms/frame \(([0-9.]+) preparing.* (\d+) draws/frame') {
        $dr = [int]$Matches[5]
        if ($dr -ne $ultimo) { $telas += "| $($Matches[1]) | $dr | $($Matches[2]) | $($Matches[4]) |"; $ultimo = $dr }
    }
}
$erros = $texto | Where-Object { $_ -match ' ERROR |panicked|E_OUTOFMEMORY|device removed|DXGI_ERROR' }
$avisos = $texto | Where-Object { $_ -match ' WARN ' } | ForEach-Object { ($_ -replace '^\S+\s+WARN\s+', '') } | Group-Object | Sort-Object Count -Descending | Select-Object -First 12
$limite = Minimo $pc "commit_limite_mb"
$picoCommit = Pico $pc "commit_mb"

$r = @(
    "# Execucao $stamp - $Nome"
    ""
    "- Duracao: $duracao s; terminou: $(if($saiuSozinho){"sozinho, codigo $codigo"}else{'encerrado pelo script (tempo maximo)'})"
    "- Commit: $commit$sujo; exe: $exe; teclas: $(if($Teclas){'sim'}else{'nao'})"
    "- Bench: $((Get-Content "$dir\bench.txt" -ErrorAction SilentlyContinue | Select-String 'bench:' | Select-Object -First 1).Line)"
    ""
    "## PC"
    "| Medida | Valor |"
    "|---|---|"
    "| CPU total media / pico | $(Media $pc 'cpu_total_pct') % / $(Pico $pc 'cpu_total_pct') % |"
    "| CPU do jogo media / pico (100% = todos os nucleos) | $(Media $pc 'cpu_processo_pct') % / $(Pico $pc 'cpu_processo_pct') % |"
    "| RAM livre minima | $(Minimo $pc 'ram_livre_mb') MB |"
    "| Commit pico / limite | $picoCommit MB / $limite MB |"
    "| Memoria privada do jogo pico | $(Pico $pc 'processo_privado_mb') MB |"
    "| Paginas lidas do disco pico | $(Pico $pc 'paginas_lidas_s') /s |"
    "| Leitura de disco pico | $(Pico $pc 'disco_leitura_mb_s') MB/s |"
)
if ($gpu.Count -gt 0) {
    $r += @(
        "| GPU uso medio / pico | $(Media $gpu 'util') % / $(Pico $gpu 'util') % |"
        "| VRAM usada pico (toda a placa) | $(Pico $gpu 'mem') MiB de 4096 |"
        "| Temperatura pico | $(Pico $gpu 'temp') C |"
        "| Consumo pico | $(Pico $gpu 'power') W |"
        "| Clock grafico min / max | $(Minimo $gpu 'clock') / $(Pico $gpu 'clock') MHz |"
    )
}
$alertas = @()
if ($limite -and $picoCommit -and $picoCommit -gt 0.9 * $limite) { $alertas += "- **Commit chegou a $picoCommit de $limite MB**: aumentar o pagefile (plano: 32-48 GB no SSD)." }
if ((Minimo $pc 'ram_livre_mb') -lt 800) { $alertas += "- **RAM livre caiu abaixo de 800 MB**: o Windows esta paginando." }
if ($gpu.Count -gt 0 -and (Pico $gpu 'mem') -gt 3700) { $alertas += "- **VRAM perto do limite de 4 GB**." }
if ($gpu.Count -gt 0 -and (Pico $gpu 'temp') -gt 83) { $alertas += "- **GPU acima de 83 C**: pode estar perdendo clock por temperatura." }
if ($erros) { $alertas += "- **$(@($erros).Count) linhas de erro no log** (abaixo)." }
if ($alertas) { $r += @("", "## Alertas") + $alertas }
$r += @("", "## Telas (cada vez que os draws por quadro mudaram)", "| Hora (UTC) | Draws/quadro | FPS | Preparing ms |", "|---|---|---|---|") + $telas
if ($erros) { $r += @("", "## Erros (primeiros 15)", '```') + ($erros | Select-Object -First 15 | ForEach-Object { $_.Substring(0, [Math]::Min(300, $_.Length)) }) + '```' }
$r += @("", "## Avisos mais frequentes", '```') + ($avisos | ForEach-Object { "$($_.Count)x $($_.Name.Substring(0, [Math]::Min(200, $_.Name.Length)))" }) + '```'
if ($Captura) { $r += @("", "## Capturas", "$(@(Get-ChildItem (Join-Path $dir 'capturas') -Filter *.png -ErrorAction SilentlyContinue).Count) imagens em capturas\ (a cada 300 quadros e nas potencias de 2).") }
$r | Set-Content -Encoding UTF8 (Join-Path $dir "resumo.md")
Write-Host "Pronto: $dir\resumo.md"
