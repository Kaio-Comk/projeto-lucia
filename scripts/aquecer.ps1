# Projeto Lucia - pre-traduz os shaders do mundo (so CPU), com o MESMO perfil do jogo.
# Sem isto, a 1a entrada no mundo compila ~1.600 pipelines de uma vez e o watchdog
# do proprio jogo derruba o processo (visto em 26/11).
#
# Uso (na pasta lucia):  .\scripts\aquecer.ps1            (hashes de todos os runs\*\jogo.log)
#                        .\scripts\aquecer.ps1 -Tudo      (todos os shaders do jogo; demora horas)
# Depois de mudar o codigo do tradutor ou o perfil, rode de novo.

param(
    [switch]$Tudo,
    [switch]$Diag,
    [int]$Jobs = 6,
    [string]$Jogo = ""   # padrao: ..\GTA6 (a pasta do jogo ao lado do repositorio)
)
$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent $PSScriptRoot
Set-Location $repo
if (-not $Jogo) { $Jogo = Join-Path (Split-Path -Parent $repo) "GTA6" }
. (Join-Path $PSScriptRoot "perfil-pc4gb.ps1")
foreach ($k in $PerfilPc4gb.Keys) { Set-Item -Path "Env:$k" -Value $PerfilPc4gb[$k] }
# O que o jogo diz do host nesta placa (log: "host wave lanes min=32", "host view heaps hold 1000000 views").
$env:VICTORY_HOST_WAVE = "32"
$env:VICTORY_VIEW_LIMIT = "1000000"

$exe = if ($Diag) { "target\diag\release\victory.exe" } else { "target\release\victory.exe" }
$runs = Join-Path (Split-Path -Parent $repo) "runs"
if ($Tudo) {
    $lista = "all"
} else {
    $lista = Join-Path $runs "hashes-todos.txt"
    $set = New-Object 'System.Collections.Generic.HashSet[string]'
    Get-ChildItem $runs -Directory | ForEach-Object {
        $log = Join-Path $_.FullName "jogo.log"
        if (Test-Path $log) {
            Select-String -Path $log -Pattern 'shader translated .* hash=([0-9a-f]{16})|shaders=\[(?:\w+ )?([0-9a-f]{16})' -AllMatches | ForEach-Object {
                foreach ($m in $_.Matches) { foreach ($g in 1, 2) { if ($m.Groups[$g].Success) { [void]$set.Add($m.Groups[$g].Value) } } }
            }
        }
    }
    $set | Set-Content $lista
    Write-Host "Shaders conhecidos dos logs: $($set.Count)"
}
$sw = [Diagnostics.Stopwatch]::StartNew()
# The translator writes notes to stderr; in Windows PowerShell 5.1 that is an
# error record, which "Stop" would turn into an abort.
$ErrorActionPreference = "Continue"
& $exe translate --warm $lista --jobs $Jobs $Jogo 2>&1 | ForEach-Object { "$_" } | Where-Object { $_ -notmatch 'warm-up      found ' }
Write-Host "Aquecimento em $([math]::Round($sw.Elapsed.TotalMinutes, 1)) min ($exe)"
