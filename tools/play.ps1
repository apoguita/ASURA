<#
.SYNOPSIS
    Start the whole stack for a real client: seed, auth service, world server.

.DESCRIPTION
    Three things have to be true before a client can get from the login screen into a map, and this
    script makes all three true in one command:

      1. the database has the dev identities the patched client presents, and the maps a world server
         can host (tools/seed-dev.ps1, tools/seed-map.ps1);
      2. the auth service is listening on 6112 and answering logins;
      3. a world server is listening on 6113, registered, publishing a pool for the map, and answering
         the game connection.

    It leaves both services running and tails their output into this console, so every message in both
    directions shows up here with its size. Ctrl+C stops both.

    **It does not start the database or the web gate.** PostgreSQL is a Windows service (a stopped one
    refuses to connect, in the services' own log rather than on screen), and a client launched with
    `-portal 127.0.0.1` asks the reference web gate on 6601 who it is *before* it logs in -- with
    nothing there it opens the auth connection, sends its preamble, gets its salt, and closes without
    ever logging in, which looks exactly like a server that is not running. Use
    `tools/start-server.ps1` to bring up all of it and `tools/stop-server.ps1` to take it down; this
    script is the piece of that pair which runs the services and shows their trace.

    The client is launched by you, not by this script: it needs a window, and a game window cannot be
    read by a script. The services' output is the verdict.

    ASCII only, like every script in tools/.

.EXAMPLE
    .\tools\start-server.ps1

.EXAMPLE
    .\tools\play.ps1

.EXAMPLE
    # a world that hosts both pre-Searing outposts
    .\tools\play.ps1 -Maps 146,148

.EXAMPLE
    # every message both ways, with its header and size -- what a client bug gets diagnosed with
    .\tools\play.ps1 -Trace
#>

param(
    [string] $Maps = "146",
    [int] $World = 8001,
    [int] $Capacity = 8,
    # Per-message tracing is OFF by default. A world tick every 500 ms and a ping every 5 s means a traced
    # session prints about three lines a second forever, which buries the lines that matter (who joined,
    # what they dropped, what the server refused) under a packet log nobody reads. `-Trace` turns it on
    # for the times a packet log is exactly what is wanted.
    [switch] $Trace,
    # The old spelling, kept so an existing command line does not break. It is the default now.
    [switch] $Quiet,
)

$ErrorActionPreference = "Stop"

$repo = Split-Path -Parent $PSScriptRoot

function Find-Cargo {
    $onPath = Get-Command cargo -ErrorAction SilentlyContinue
    if ($onPath) { return $onPath.Source }
    $candidate = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
    if (Test-Path $candidate) { return $candidate }
    throw "cargo not found. Install Rust, or add %USERPROFILE%\.cargo\bin to PATH."
}

function Invoke-Step {
    param([string] $Script, [string] $Label)
    Write-Host "== $Label"
    # The child's output is passed through rather than captured into `$code`: PowerShell would then
    # compare its stdout text against 0 and call a successful step a failure.
    $previous = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    try {
        & (Get-Command powershell).Source -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot $Script) 2>&1 |
            ForEach-Object { Write-Host "$_" }
        $code = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $previous
    }
    if ($code -ne 0) { throw "$Label failed (exit $code)" }
}

$cargo = Find-Cargo
Write-Host "cargo: $cargo"
Write-Host ""

Invoke-Step -Script "seed-dev.ps1" -Label "seeding the identities the client presents"
Write-Host ""
Invoke-Step -Script "seed-map.ps1" -Label "seeding the maps a world can host"
Write-Host ""

Write-Host "== building"
# Cargo writes its progress to stderr, and PowerShell turns native stderr into error records that
# $ErrorActionPreference = "Stop" treats as fatal -- a successful build would abort this script.
$previous = $ErrorActionPreference
$ErrorActionPreference = "Continue"
try {
    & $cargo build --offline -p asura-auth -p asura-world 2>&1 |
        Select-String -Pattern 'error|warning: unused' |
        ForEach-Object { Write-Host "$_" }
    $buildCode = $LASTEXITCODE
} finally {
    $ErrorActionPreference = $previous
}
if ($buildCode -ne 0) { throw "the build failed (exit $buildCode)" }

$authExe = Join-Path $repo "target\debug\asura-auth.exe"
$worldExe = Join-Path $repo "target\debug\asura-world.exe"
foreach ($exe in @($authExe, $worldExe)) {
    if (-not (Test-Path $exe)) { throw "$exe is missing; the build did not produce it" }
}

$env:ASURA_TRACE = if ($Trace) { "1" } else { "0" }
$authLog = Join-Path $env:TEMP "asura-auth.log"
$worldLog = Join-Path $env:TEMP "asura-world.log"
Remove-Item $authLog, $worldLog -ErrorAction SilentlyContinue

Write-Host ""
if ($Trace) {
    Write-Host "per-message trace ON: every packet both ways, a tick every 500 ms and a ping every 5 s"
} else {
    Write-Host "per-message trace off: events only (who joined, what they did, what was refused). -Trace for packets"
}
Write-Host "chat: /maps, /stuck and a spoken line are answered here in this console -- where the reference's /maps worked too"
Write-Host "== auth service on 127.0.0.1:6112"
$auth = Start-Process -FilePath $authExe -ArgumentList "--bind", "127.0.0.1:6112" -NoNewWindow -PassThru -RedirectStandardError $authLog
Start-Sleep -Milliseconds 700

Write-Host "== world server id $World on 127.0.0.1:6113, maps $Maps"
$worldArgs = @("--world", "$World", "--maps", $Maps, "--capacity", "$Capacity", "--public", "127.0.0.1:6113")
$worldProc = Start-Process -FilePath $worldExe -ArgumentList $worldArgs -NoNewWindow -PassThru -RedirectStandardError $worldLog
Start-Sleep -Milliseconds 700

Write-Host ""
Write-Host "client command line:"
Write-Host "  F:\GW\GW1\Gw.custom.exe -authsrv 127.0.0.1 -portal 127.0.0.1"
Write-Host ""
Write-Host "the service output follows. What a successful travel looks like:"
Write-Host "  auth:  login ok for ... with 1 character(s)"
Write-Host "  auth:  travel to map 146 placed on world $World"
Write-Host "  world: joined map 146 ... with 6 bag(s) and 9 item(s)"
if ($Trace) {
    Write-Host "  world: 'world -> 0x018E' -- INSTANCE_LOAD_FINISH, which ends the loading screen"
}
Write-Host ""
Write-Host "Ctrl+C stops both services."
Write-Host ""

# Tail both logs into this console: the trace is the verdict, and a file the user has to remember to
# open is a trace nobody reads.
$authSeen = 0
$worldSeen = 0
try {
    while ($true) {
        Start-Sleep -Milliseconds 250
        if ($auth.HasExited) { throw "the auth service exited (see $authLog)" }
        if ($worldProc.HasExited) { throw "the world server exited (see $worldLog)" }

        foreach ($pair in @(@($authLog, "auth", $authSeen), @($worldLog, "world", $worldSeen))) {
            $path = $pair[0]
            $tag = $pair[1]
            $seen = [int] $pair[2]
            if (-not (Test-Path $path)) { continue }
            $lines = @(Get-Content -Path $path -ErrorAction SilentlyContinue)
            if ($lines.Count -gt $seen) {
                for ($index = $seen; $index -lt $lines.Count; $index++) {
                    Write-Host ("[{0}] {1}" -f $tag, $lines[$index])
                }
                if ($tag -eq "auth") { $authSeen = $lines.Count } else { $worldSeen = $lines.Count }
            }
        }
    }
} finally {
    Write-Host ""
    Write-Host "stopping"
    if (-not $worldProc.HasExited) { Stop-Process -Id $worldProc.Id -Force -ErrorAction SilentlyContinue }
    if (-not $auth.HasExited) { Stop-Process -Id $auth.Id -Force -ErrorAction SilentlyContinue }
}
