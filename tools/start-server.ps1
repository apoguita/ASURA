<#
.SYNOPSIS
    Start everything a real client needs, in the order it needs them: database, web gate, services.

.DESCRIPTION
    `tools/play.ps1` starts the two services and seeds what they read. Two things it does NOT do are on
    this script, because a client cannot log in without them, and both failures look the same from the
    client: "cannot connect".

      1. **PostgreSQL.** It is a Windows service on this machine. If it is stopped, nothing connects --
         and the services refuse to start, with a message in their own log rather than on screen.
         Starting a service needs elevation, so this script says so instead of failing obscurely.

      2. **The reference web gate on 6601** (`PreSearing/tools/webgate.py`). With `-portal 127.0.0.1`
         the client asks that HTTP service who it is -- `session/create.xml`, `users/login.xml`,
         `my_account/game_accounts.xml`, `my_account/token.xml` -- and only then sends
         `AUTH_CMSG_PORTAL_ACCOUNT_LOGIN` over the socket it already opened. With nothing listening on
         6601 it opens the auth connection, sends its preamble, gets its session salt, and then closes
         without ever logging in. That is what a "server is not running" client error actually looked
         like on this machine, with the server running perfectly.

    Then it hands over to `tools/play.ps1`: seed the dev identities and maps, build, start `asura-auth`
    on 6112 and `asura-world` on 6113, tail both logs. Ctrl+C stops the services and this script stops
    the web gate it started.

    ASCII only, like every script in tools/.

.PARAMETER Maps
    Comma-separated map ids the world hosts. Default 146 (Lakeside County).

.PARAMETER World
    The world id the servers register under. Default 8001.

.PARAMETER Capacity
    Slots per map. Default 8.

.PARAMETER Trace
    Passed to play.ps1: print every packet both ways. Off by default, because a world tick every 500 ms
    makes a traced console three lines a second of packet log with the events buried in it.

.PARAMETER PreSearing
    The reference checkout holding tools/webgate.py. Default: the sibling of this repository.

.PARAMETER GatePort
    The port the web gate listens on. Default 6601, which is what the client calls.

.PARAMETER SkipDatabase
    Do not touch the PostgreSQL service (for when it is managed elsewhere).

.EXAMPLE
    .\tools\start-server.ps1

.EXAMPLE
    # every packet, for diagnosing a client
    .\tools\start-server.ps1 -Trace

.EXAMPLE
    # both pre-Searing outposts, and the reference checkout somewhere else
    .\tools\start-server.ps1 -Maps 146,148 -PreSearing 'D:\PreSearing'
#>

param(
    [string] $Maps = "146",
    [int]    $World = 8001,
    [int]    $Capacity = 8,
    [switch] $Trace,
    # The old spelling; per-message tracing is off by default now.
    [switch] $Quiet,
    [string] $PreSearing = '',
    [int]    $GatePort = 6601,
    [switch] $SkipDatabase
)

$ErrorActionPreference = "Stop"

$repo = Split-Path -Parent $PSScriptRoot

function Test-Port {
    param([int] $Port)
    try {
        return [bool] (Get-NetTCPConnection -LocalPort $Port -State Listen -ErrorAction SilentlyContinue)
    } catch {
        # Older hosts without Get-NetTCPConnection still have .NET's TcpListener; a failed bind means the
        # port is taken, which is the same question.
        try {
            $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, $Port)
            $listener.Start()
            $listener.Stop()
            return $false
        } catch {
            return $true
        }
    }
}

function Write-State {
    param([string] $What, [string] $State)
    Write-Host ("  {0,-28} {1}" -f $What, $State)
}

Write-Host "ASURA: starting everything a client needs"
Write-Host ""

# ---- 1. PostgreSQL ------------------------------------------------------------------------------
Write-Host "== 1. database"
if ($SkipDatabase) {
    Write-Host "  skipped (-SkipDatabase)"
} elseif (Test-Port 5432) {
    Write-State "PostgreSQL :5432" "already listening"
} else {
    $service = Get-Service -Name "postgresql*" -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $service) {
        Write-Host "  no PostgreSQL service found. Install it, or run it somewhere this machine can reach" -ForegroundColor Yellow
        Write-Host "  and point ASURA_DATABASE_URL at it (see .env.example)." -ForegroundColor Yellow
        exit 2
    }
    Write-Host "  service '$($service.Name)' is $($service.Status); starting it"
    try {
        Start-Service -Name $service.Name
    } catch {
        Write-Host "  could not start the service: $($_.Exception.Message)" -ForegroundColor Yellow
        Write-Host "  Starting a Windows service needs an elevated shell:" -ForegroundColor Yellow
        Write-Host "    Start-Process powershell -Verb RunAs -ArgumentList '-NoProfile','-Command','Start-Service $($service.Name)'" -ForegroundColor Yellow
        exit 2
    }
    for ($i = 0; $i -lt 40 -and -not (Test-Port 5432); $i++) { Start-Sleep -Milliseconds 250 }
    if (-not (Test-Port 5432)) {
        Write-Host "  the service started but nothing is listening on 5432" -ForegroundColor Yellow
        exit 2
    }
    Write-State "PostgreSQL :5432" "started"
}

# ---- 2. the reference web gate ------------------------------------------------------------------
Write-Host ""
Write-Host "== 2. web gate (the client asks this who it is before it logs in)"
$gate = $null
if (Test-Port $GatePort) {
    Write-State "web gate :$GatePort" "already listening"
} else {
    if (-not $PreSearing) {
        $PreSearing = Join-Path (Split-Path -Parent $repo) "PreSearing"
    }
    $gateScript = Join-Path $PreSearing "tools\webgate.py"
    if (-not (Test-Path $gateScript)) {
        Write-Host "  no web gate at $gateScript" -ForegroundColor Yellow
        Write-Host "  The client launched with -portal needs it. Either point -PreSearing at the checkout" -ForegroundColor Yellow
        Write-Host "  that holds tools/webgate.py, or start the client without -portal." -ForegroundColor Yellow
        exit 3
    }
    $python = (Get-Command python -ErrorAction SilentlyContinue).Source
    if (-not $python) {
        Write-Host "  python is not on PATH; it is what runs the web gate" -ForegroundColor Yellow
        exit 3
    }
    $gateOut = Join-Path $repo ".webgate.log"
    $gateErr = Join-Path $repo ".webgate.err"
    Remove-Item $gateOut, $gateErr -ErrorAction SilentlyContinue
    Write-Host "  starting $gateScript"
    $gate = Start-Process -FilePath $python -ArgumentList $gateScript `
        -WorkingDirectory (Split-Path -Parent $gateScript) `
        -WindowStyle Hidden -PassThru `
        -RedirectStandardOutput $gateOut -RedirectStandardError $gateErr
    for ($i = 0; $i -lt 40 -and -not (Test-Port $GatePort); $i++) { Start-Sleep -Milliseconds 250 }
    if (-not (Test-Port $GatePort)) {
        Write-Host "  the web gate did not come up on $GatePort" -ForegroundColor Yellow
        if (Test-Path $gateErr) { Get-Content $gateErr -Tail 8 | ForEach-Object { Write-Host "    $_" } }
        if ($gate -and -not $gate.HasExited) { Stop-Process -Id $gate.Id -Force -ErrorAction SilentlyContinue }
        exit 3
    }
    Write-State "web gate :$GatePort" "started (pid $($gate.Id))"
}

try {
    # ---- 3. the services ------------------------------------------------------------------------
    Write-Host ""
    foreach ($port in 6112, 6113) {
        if (Test-Port $port) {
            Write-Host "  port $port is already in use -- an earlier run is probably still up." -ForegroundColor Yellow
            Write-Host "  Stop it first:  Get-Process asura-auth,asura-world | Stop-Process -Force" -ForegroundColor Yellow
            exit 4
        }
    }
    Write-Host "== 3. the services (play.ps1: seed, build, auth :6112, world :6113)"
    Write-Host ""
    $playArgs = @("-NoProfile", "-ExecutionPolicy", "Bypass", "-File", (Join-Path $PSScriptRoot "play.ps1"),
        "-Maps", $Maps, "-World", "$World", "-Capacity", "$Capacity")
    if ($Trace) { $playArgs += "-Trace" }
    & (Get-Command powershell).Source @playArgs
    $code = $LASTEXITCODE
    if ($code -ne 0) {
        Write-Host ""
        Write-Host "play.ps1 exited with $code" -ForegroundColor Yellow
    }
} finally {
    if ($gate -and -not $gate.HasExited) {
        Stop-Process -Id $gate.Id -Force -ErrorAction SilentlyContinue
        Write-Host "web gate stopped"
    }
}
