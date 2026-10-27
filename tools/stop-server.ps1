<#
.SYNOPSIS
    Stop everything tools/start-server.ps1 started: the two services, and the web gate on 6601.

.DESCRIPTION
    The counterpart of `tools/start-server.ps1`. It is deliberately blunt -- a hard stop, in this order:

      1. the world server (`asura-world`) and the auth service (`asura-auth`);
      2. the web gate listening on 6601, unless -KeepGate;
      3. optionally the PostgreSQL service, with -Database (that one needs elevation, and is OFF by
         default because stopping a database other things may be using is not this script's decision).

    Then it checks the ports are actually free and says which process still holds any that are not,
    because "I stopped it" and "the port is free" are different claims and the second is the one that
    matters when the next start fails to bind.

    **One thing a hard stop leaves behind, stated rather than hidden**: a player who is connected when
    the world is killed never gets the release-on-leave path, so that player's pool slot stays leased
    until something calls the reaper (`asura_db::placement::reap_stale`, which nothing schedules yet).
    With the default capacity of 8 one stale slot does not refuse a login; the script prints the pools
    that are leased so the number is visible rather than guessed at.

    ASCII only, like every script in tools/.

.PARAMETER KeepGate
    Leave the web gate on 6601 running (it is harmless without the services, and starting it costs a
    second).

.PARAMETER Database
    Also stop the PostgreSQL Windows service. Needs an elevated shell.

.PARAMETER Quiet
    Only the summary.

.EXAMPLE
    .\tools\stop-server.ps1

.EXAMPLE
    # leave the web gate alone, and stop the database too
    .\tools\stop-server.ps1 -KeepGate -Database
#>

param(
    [switch] $KeepGate,
    [switch] $Database,
    [switch] $Quiet
)

$ErrorActionPreference = "Stop"

$repo = Split-Path -Parent $PSScriptRoot
$script:Remaining = New-Object System.Collections.ArrayList

function Write-State {
    param([string] $What, [string] $State, [string] $Colour = "Gray")
    Write-Host ("  {0,-24} {1}" -f $What, $State) -ForegroundColor $Colour
}

function Get-PortOwner {
    param([int] $Port)
    try {
        $connection = Get-NetTCPConnection -LocalPort $Port -State Listen -ErrorAction SilentlyContinue |
            Select-Object -First 1
        if (-not $connection) { return $null }
        $process = Get-Process -Id $connection.OwningProcess -ErrorAction SilentlyContinue
        return [pscustomobject]@{ Pid = $connection.OwningProcess; Name = $process.Name }
    } catch {
        return $null
    }
}

Write-Host "ASURA: stopping"
Write-Host ""

# ---- 1. the services ----------------------------------------------------------------------------
Write-Host "== services"
foreach ($name in "asura-world", "asura-auth") {
    $processes = @(Get-Process -Name $name -ErrorAction SilentlyContinue)
    if ($processes.Count -eq 0) {
        Write-State $name "not running"
        continue
    }
    foreach ($process in $processes) {
        if (-not $Quiet) { Write-Host "  stopping $name (pid $($process.Id))" }
        Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
    }
    # Wait for it to actually go: a kill is a request, and the next line checks the result.
    for ($i = 0; $i -lt 20 -and (Get-Process -Name $name -ErrorAction SilentlyContinue); $i++) {
        Start-Sleep -Milliseconds 100
    }
    $left = @(Get-Process -Name $name -ErrorAction SilentlyContinue)
    if ($left.Count -eq 0) {
        Write-State $name "stopped"
    } else {
        Write-State $name "STILL RUNNING (pids $($left.Id -join ', '))" "Yellow"
        [void] $script:Remaining.Add($name)
    }
}

# ---- 2. the web gate ----------------------------------------------------------------------------
Write-Host ""
Write-Host "== web gate"
$gate = Get-PortOwner 6601
if ($KeepGate) {
    Write-State "web gate :6601" $(if ($gate) { "left running (pid $($gate.Pid))" } else { "not running" })
} elseif ($gate) {
    if (-not $Quiet) { Write-Host "  stopping $($gate.Name) (pid $($gate.Pid)) on 6601" }
    Stop-Process -Id $gate.Pid -Force -ErrorAction SilentlyContinue
    for ($i = 0; $i -lt 20 -and (Get-PortOwner 6601); $i++) { Start-Sleep -Milliseconds 100 }
    if (Get-PortOwner 6601) {
        Write-State "web gate :6601" "STILL LISTENING" "Yellow"
        [void] $script:Remaining.Add("web gate")
    } else {
        Write-State "web gate :6601" "stopped"
    }
} else {
    # Not on 6601: a webgate started on another port would still be a stray. Only one is expected here.
    $strays = @(Get-CimInstance Win32_Process -Filter "Name = 'python.exe'" -ErrorAction SilentlyContinue |
        Where-Object { $_.CommandLine -like "*webgate.py*" })
    if ($strays.Count -eq 0) {
        Write-State "web gate" "not running"
    } else {
        foreach ($stray in $strays) {
            if (-not $Quiet) { Write-Host "  stopping a stray web gate (pid $($stray.ProcessId))" }
            Stop-Process -Id $stray.ProcessId -Force -ErrorAction SilentlyContinue
        }
        Write-State "web gate" "stopped $($strays.Count) straggler(s)"
    }
}

# ---- 3. the database, only when asked -----------------------------------------------------------
Write-Host ""
Write-Host "== database"
if ($Database) {
    $service = Get-Service -Name "postgresql*" -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $service) {
        Write-State "PostgreSQL" "no service found"
    } elseif ($service.Status -ne "Running") {
        Write-State "PostgreSQL" "already stopped"
    } else {
        try {
            Stop-Service -Name $service.Name
            Write-State "PostgreSQL" "stopped ($($service.Name))"
        } catch {
            Write-State "PostgreSQL" "could not stop: needs an elevated shell" "Yellow"
            if (-not $Quiet) {
                Write-Host "    Start-Process powershell -Verb RunAs -ArgumentList '-NoProfile','-Command','Stop-Service $($service.Name)'" -ForegroundColor Yellow
            }
            [void] $script:Remaining.Add("PostgreSQL")
        }
    }
} else {
    Write-State "PostgreSQL" "left running (use -Database to stop it)"
}

# ---- 4. are the ports actually free? ------------------------------------------------------------
Write-Host ""
Write-Host "== ports"
foreach ($port in 6112, 6113, 6601) {
    if ($KeepGate -and $port -eq 6601) {
        Write-State ":$port" "left as it was"
        continue
    }
    $owner = Get-PortOwner $port
    if ($owner) {
        Write-State ":$port" "STILL LISTENING ($($owner.Name), pid $($owner.Pid))" "Yellow"
        [void] $script:Remaining.Add("port $port")
    } else {
        Write-State ":$port" "free"
    }
}

# ---- 5. what a hard stop leaves behind ----------------------------------------------------------
Write-Host ""
Write-Host "note: a player connected when the world was killed keeps its pool slot leased until the reaper"
Write-Host "      runs (asura_db::placement::reap_stale, which nothing schedules yet). It does not refuse a"
Write-Host "      login at the default capacity of 8."

Write-Host ""
if ($script:Remaining.Count -eq 0) {
    Write-Host "stopped: nothing of ASURA is still listening" -ForegroundColor Green
    exit 0
}
Write-Host ("not stopped: {0}" -f ($script:Remaining -join ", ")) -ForegroundColor Red
exit 1
