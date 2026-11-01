<#
.SYNOPSIS
    One run that records the stream and dumps the client structures, so a set of performed
    actions can be attributed to messages and to state.

.DESCRIPTION
    Starts tools/sniff-capture.py in the background (both directions, raw, names kept: that file
    stays OUT of the repository), then dumps the client's contexts on a cadence with
    tools/context-dump.py, and lets the operator mark each action as it is performed.

    Press Enter after each action. Marker n belongs to action n in the list printed at start, and
    the marker carries the wall clock time, so the dump pair around it and the capture window
    around it are the evidence for that one action.

    Everything needs an elevated token: the library opens the client with write rights even for
    reads. The script relaunches itself elevated once, so approve the UAC prompt.

.PARAMETER Minutes
    How long to record. The sniffer stops itself at this mark; dumps stop at the same time.

.PARAMETER DumpEverySeconds
    Cadence of the structure dumps. Twenty seconds leaves roughly ninety dumps in half an hour.

.PARAMETER OutDir
    Where the capture, the dumps and the log go. Outside the repository by default, because a raw
    capture carries names and UUIDs.

.EXAMPLE
    .\tools\live-session.ps1 -Minutes 30
#>
param(
    [ValidateSet('init', 'identify')]
    [string]$Preset = 'init',
    [int]$Minutes = 20,
    [int]$DumpEverySeconds = 0,
    [string]$Only = '',
    [string]$OutDir = 'C:\Users\Apo\captures\session',
    [switch]$Elevated
)

$ErrorActionPreference = 'Continue'

# --- actions this run is for, in order: marker n belongs to item n -----------------------------
$presets = @{
    init = @(
        'enter an outpost from character select',
        'travel from that outpost to the explorable it exits to',
        'come back to the outpost',
        'change district inside that outpost',
        'open the world map, then close it',
        'walk to a portal and cross it'
    )
    identify = @(
    'toggle hard mode in an explorable (party panel)',
    'add one henchman in an outpost, then dismiss it',
    'accept one quest at a quest NPC',
    'die, then get resurrected',
    'kill one foe',
    'drop one item, then pick it up',
    'summon something (pet, minion or spirit)',
    'dye one equipped item',
    'click the eye on one inventory slot (display mode)',
    'enter the guild hall, or an outpost your guild holds',
    'open the world map, click a destination, close it',
    'trigger a cinematic',
    'press one adrenal skill on a warrior with adrenaline',
    'travel to an outpost with a second player in it',
        'talk to a merchant, buy nothing, then buy one thing'
    )
}
$actions = $presets[$Preset]
if ($DumpEverySeconds -le 0) { $DumpEverySeconds = if ($Preset -eq 'init') { 8 } else { 20 } }
if (-not $Only -and $Preset -eq 'init') {
    $Only = 'worldcontext,mapcontext,charcontext,partycontext,itemcontext,agentarray,accagentcontext,instanceinfo,missionmap,worldmap,serverregion'
}

function Test-Admin {
    $id = [Security.Principal.WindowsIdentity]::GetCurrent()
    return (New-Object Security.Principal.WindowsPrincipal($id)).IsInRole(
        [Security.Principal.WindowsBuiltInRole]::Administrator)
}

if (-not (Test-Admin)) {
    $shell = (Get-Process -Id $PID).Path
    $quoted = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $PSCommandPath,
                '-Preset', $Preset, '-Minutes', $Minutes, '-DumpEverySeconds', $DumpEverySeconds,
                '-Only', $Only, '-OutDir', $OutDir, '-Elevated')
    Write-Host 'asking for an elevated shell: approve the UAC prompt' -ForegroundColor Yellow
    Start-Process -FilePath $shell -Verb RunAs -ArgumentList $quoted
    exit 0
}

$repo = Split-Path (Split-Path $PSCommandPath -Parent) -Parent
$capture = Join-Path $OutDir 'capture-raw.jsonl'
$dumpDir = Join-Path $OutDir 'dumps'
$log = Join-Path $OutDir 'session.log'
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
New-Item -ItemType Directory -Force -Path $dumpDir | Out-Null
if (Test-Path $capture) { Remove-Item $capture -Force }

function Say([string]$text, [string]$colour = 'Gray') {
    Write-Host $text -ForegroundColor $colour
    Add-Content -Path $log -Value ('[{0}] {1}' -f (Get-Date -Format 'HH:mm:ss'), $text)
}

Set-Content -Path $log -Value ('session {0}' -f (Get-Date -Format 'yyyy-MM-dd HH:mm:ss'))
Say ('repository: ' + $repo)
Say ('capture:    ' + $capture)
Say ('dumps:      ' + $dumpDir)

if (-not (Test-Path (Join-Path $repo 'tools\sniff-capture.py'))) { Say 'sniff-capture.py not found' 'Red'; exit 3 }
if (-not (Test-Path (Join-Path $repo 'tools\context-dump.py')))  { Say 'context-dump.py not found' 'Red'; exit 3 }
$python = (Get-Command python -ErrorAction SilentlyContinue)
if (-not $python) { Say 'python is not on PATH in this shell' 'Red'; exit 3 }
Say ('python: ' + $python.Source)
$client = Get-Process | Where-Object { $_.ProcessName -match '^Gw' } | Select-Object -First 1
if (-not $client) { Say 'no Guild Wars client is running' 'Red'; exit 3 }
Say ('client: pid {0} {1}' -f $client.Id, $client.ProcessName)
Say ('preset: {0}, dumps every {1}s' -f $Preset, $DumpEverySeconds)

Write-Host ''
Write-Host 'the actions, in order - press Enter after each one:' -ForegroundColor Cyan
for ($i = 0; $i -lt $actions.Count; $i++) { Write-Host ('  {0,2}. {1}' -f ($i + 1), $actions[$i]) }
Write-Host ''
Write-Host 'starting the sniffer, then dumping every ' -NoNewline
Write-Host ('{0}s' -f $DumpEverySeconds) -NoNewline
Write-Host '. Ctrl+C stops it; the files stay.'
Write-Host ''

$seconds = $Minutes * 60
Set-Location $repo
$sniffer = Start-Process -FilePath $python.Source -PassThru -WindowStyle Hidden -ArgumentList @(
    'tools\sniff-capture.py', '--dir', 'both', '--seconds', $seconds, '--raw', '--out', $capture)
Say ('sniffer started: pid {0}, {1} seconds, both directions, raw' -f $sniffer.Id, $seconds)

$deadline = (Get-Date).AddSeconds($seconds - 15)
$dumps = 0
$marks = 0
$started = Get-Date

try {
    while ((Get-Date) -lt $deadline) {
        $stamp = Get-Date -Format 'HHmmss'
        $out = Join-Path $dumpDir ('t' + $stamp + '.jsonl')
                $onlyArgs = @()
        if ($Only) { $onlyArgs = @('--only', $Only) }
        $read = & python tools\context-dump.py --label ('t' + $stamp) --out $out @onlyArgs 2>&1
        $summary = ($read | Where-Object { $_ -match 'context\(s\) read' } | Select-Object -Last 1)
        if (-not $summary) { $summary = ($read | Select-Object -Last 1) }
        $dumps++
        Say ('dump {0,3} t{1}: {2}' -f $dumps, $stamp, ([string]$summary).Trim())

        $spent = ((Get-Date) - $started).TotalSeconds
        if ($spent -gt $seconds - 20) { break }

        $waitUntil = (Get-Date).AddSeconds($DumpEverySeconds)
        while ((Get-Date) -lt $waitUntil) {
            if ([Console]::KeyAvailable) {
                $key = [Console]::ReadKey($true)
                if ($key.Key -eq 'Enter') {
                    $marks++
                    $text = if ($marks -le $actions.Count) { $actions[$marks - 1] } else { 'extra mark' }
                    Say ('MARK {0} at {1} - {2}' -f $marks, (Get-Date -Format 'HH:mm:ss'), $text) 'Green'
                }
            }
            Start-Sleep -Milliseconds 200
        }
    }
}
finally {
    if ($sniffer -and -not $sniffer.HasExited) {
        Say 'waiting for the sniffer to finish its window'
        try { $sniffer.WaitForExit(30000) | Out-Null } catch { }
        if (-not $sniffer.HasExited) { try { Stop-Process -Id $sniffer.Id -Force } catch { } ; Say 'sniffer stopped early' }
    }
}

$manifest = [ordered]@{
    started = $started.ToString('o')
    ended = (Get-Date).ToString('o')
    capture = $capture
    dumps = $dumpDir
    dump_count = $dumps
    marks = $marks
    actions = $actions
    note = 'marker n belongs to action n; the dump pair around a marker and the capture window around it are the evidence'
}
$manifestPath = Join-Path $OutDir 'manifest.json'
$manifest | ConvertTo-Json -Depth 4 | Set-Content -Path $manifestPath -Encoding utf8
Say ('done: {0} dump(s), {1} mark(s)' -f $dumps, $marks) 'Cyan'
Say ('manifest: ' + $manifestPath)
Write-Host ''
Write-Host 'hand back these two paths:' -ForegroundColor Cyan
Write-Host ('  ' + $capture)
Write-Host ('  ' + $dumpDir)