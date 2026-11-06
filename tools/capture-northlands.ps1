# Capture the live client's Lakeside -> Northlands -> Lakeside packets.
# Run from an elevated PowerShell. The existing sniffer selects 32-bit Python and owns cleanup.
# Usage: .\tools\capture-northlands.ps1
# With several clients: .\tools\capture-northlands.ps1 -ClientPid 1234
# Optional fixed capture length: -Seconds 180 (default: stop with Ctrl+C).
param(
    [int]$ClientPid = 0,
    [double]$Seconds = 0,
    [string]$Python = 'python',
    [string]$StealthRoot = ''
)

$ErrorActionPreference = 'Stop'
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = New-Object Security.Principal.WindowsPrincipal($identity)
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw 'Run this script from PowerShell opened as administrator.'
}
if ($Seconds -lt 0) { throw 'Seconds must be zero (Ctrl+C) or positive.' }

$repoRoot = Split-Path -Parent $PSScriptRoot
$sniffer = Join-Path $PSScriptRoot 'sniff-capture.py'
if (-not (Test-Path -LiteralPath $sniffer)) { throw "Missing $sniffer" }
if ($StealthRoot -eq '') {
    $StealthRoot = if ($env:PY4GW_STEALTH) { $env:PY4GW_STEALTH } else { 'C:\Users\Apo\Py4GW_Stealth' }
}
if (-not (Test-Path -LiteralPath (Join-Path $StealthRoot 'py4gw'))) {
    throw "Py4GW Stealth not found in $StealthRoot; pass -StealthRoot PATH."
}
if ($ClientPid -eq 0) {
    $clients = @(Get-Process -Name 'Gw*' -ErrorAction SilentlyContinue)
    if ($clients.Count -ne 1) {
        $clients | Select-Object Id, ProcessName, Path | Format-Table | Out-Host
        throw 'Select the live client explicitly with -ClientPid NUMBER.'
    }
    $ClientPid = $clients[0].Id
}
$client = Get-Process -Id $ClientPid
Write-Host "Capturing $($client.ProcessName), PID $ClientPid, $($client.Path)"

$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$output = Join-Path $repoRoot "recordings\client\raw\northlands-live-$stamp.jsonl"
$captureArgs = @(
    $sniffer, '--pid', "$ClientPid", '--dir', 'both', '--raw',
    '--seconds', "$Seconds", '--out', $output, '--py4gw-root', $StealthRoot
)
Write-Host 'Start in Lakeside County on the live server.'
Write-Host 'Wait for the --- capturing banner, then walk into Northlands.'
Write-Host 'After Northlands loads, move a little, then walk back to Lakeside.'
Write-Host 'Once Lakeside loads, press Ctrl+C to finish and restore the sniffer.'
Write-Host "Output: $output"
Write-Host 'Full packet data includes names and UUIDs; this raw directory is excluded from Git.'
& $Python @captureArgs
exit $LASTEXITCODE
