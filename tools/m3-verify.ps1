<#
.SYNOPSIS
    Verify the login path: the schema, the identity pair, and the message table.

.DESCRIPTION
    Loads ASURA_DATABASE_URL from the environment, or from the repository's .env. Then runs:

      1. every pending migration (crates/asura-db/migrations/*.sql), applied in order. Idempotent, so
         this is safe to re-run and is how a database created before a migration catches up.
      2. the login integration tests (crates/asura-db/tests/login.rs):
         - a presented pair resolves to its account;
         - neither half of the pair is optional;
         - a closed session never resolves again, and re-seeding reopens it;
         - the newest session wins and the older one is closed;
         - a character list is scoped to its own account;
         - a character selection is recorded only on a live session.
      3. the protocol unit tests (crates/asura-protocol), including the check that every message in
         the generated table still computes to the size the GWPS reference declares.

    Harness-only: no game client. The client arrives once the handshake exists (docs/roadmap.md, M3).

.EXAMPLE
    .\tools\m3-verify.ps1

.EXAMPLE
    # also seed the rig identities the patched client presents
    .\tools\m3-verify.ps1 -Seed
#>
param(
    [switch] $Seed
)

$ErrorActionPreference = "Stop"

$repo = Split-Path -Parent $PSScriptRoot

function Find-Psql {
    $onPath = Get-Command psql -ErrorAction SilentlyContinue
    if ($onPath) { return $onPath.Source }

    $root = "C:\Program Files\PostgreSQL"
    if (Test-Path $root) {
        $candidate = Get-ChildItem $root -Directory |
            Sort-Object Name -Descending |
            ForEach-Object { Join-Path $_.FullName "bin\psql.exe" } |
            Where-Object { Test-Path $_ } |
            Select-Object -First 1
        if ($candidate) { return $candidate }
    }
    throw "psql.exe not found. Install PostgreSQL, or add its bin directory to PATH."
}

$cargo = Get-Command cargo -ErrorAction SilentlyContinue
if ($cargo) { $cargo = $cargo.Source }
if (-not $cargo) {
    $candidate = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
    if (Test-Path $candidate) { $cargo = $candidate }
}
if (-not $cargo) {
    throw "cargo not found. Install Rust, or add %USERPROFILE%\.cargo\bin to PATH."
}

function Get-DatabaseUrl {
    if ($env:ASURA_DATABASE_URL) { return $env:ASURA_DATABASE_URL.Trim() }
    $envFile = Join-Path $repo ".env"
    if (Test-Path $envFile) {
        foreach ($line in Get-Content $envFile) {
            $trimmed = $line.Trim()
            if ($trimmed -eq "" -or $trimmed.StartsWith("#")) { continue }
            $pair = $trimmed -split "=", 2
            if ($pair.Count -eq 2 -and $pair[0].Trim() -eq "ASURA_DATABASE_URL") {
                $value = $pair[1].Trim().Trim('"').Trim("'")
                if ($value) { return $value }
            }
        }
    }
    throw "ASURA_DATABASE_URL not set and no .env found. Run .\tools\db-setup.ps1 first."
}

$url = Get-DatabaseUrl
$psql = Find-Psql
Write-Host "database: $url"
Write-Host ""

# Native programs write progress and NOTICEs to stderr, and PowerShell turns native stderr into error
# records that $ErrorActionPreference = "Stop" treats as fatal. Every native call therefore runs with
# the preference relaxed and is judged by its exit code alone. Without this, a second run of an
# idempotent migration (which always prints "already exists, skipping") would abort the script, and so
# would a clean `cargo` build.
function Invoke-Native {
    param([string] $Exe, [string[]] $Arguments)
    $previous = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    try {
        & $Exe @Arguments 2>&1 | ForEach-Object { "$_" } | ForEach-Object { Write-Host $_ }
        return $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $previous
    }
}

function Invoke-PsqlFile {
    param([string] $Path)
    $previous = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    try {
        $output = & $psql -v ON_ERROR_STOP=1 -X -q -f $Path $url 2>&1 | ForEach-Object { "$_" }
        return @{ ExitCode = $LASTEXITCODE; Output = ($output -join "`n") }
    } finally {
        $ErrorActionPreference = $previous
    }
}

Write-Host "1/3  migrations"
$migrations = Get-ChildItem (Join-Path $repo "crates\asura-db\migrations") -Filter "*.sql" | Sort-Object Name
if (-not $migrations) { throw "no migrations found under crates\asura-db\migrations" }
foreach ($migration in $migrations) {
    # Options before the database URI: psql reads anything after the dbname as an extra argument.
    $result = Invoke-PsqlFile -Path $migration.FullName
    if ($result.ExitCode -ne 0) {
        throw "migration failed: $($migration.Name)`n$($result.Output)"
    }
    Write-Host "     applied $($migration.Name)"
}

Write-Host ""
Write-Host "2/3  login integration tests (identity pair, closed sessions, character scope)"
$code = Invoke-Native $cargo @("test", "--offline", "-p", "asura-db", "--test", "login", "--", "--nocapture")
if ($code -ne 0) { throw "login integration tests failed" }

Write-Host ""
Write-Host "3/3  protocol tests (the message table matches the reference)"
$code = Invoke-Native $cargo @("test", "--offline", "-p", "asura-protocol")
if ($code -ne 0) { throw "protocol tests failed" }

if ($Seed) {
    Write-Host ""
    Write-Host "seeding the rig identities the patched client presents"
    $code = Invoke-Native (Get-Command powershell).Source @("-NoProfile", "-ExecutionPolicy", "Bypass", "-File", (Join-Path $PSScriptRoot "seed-dev.ps1"))
    if ($code -ne 0) { throw "seeding failed" }
}

Write-Host ""
Write-Host "login path: OK"
Write-Host ""
Write-Host "next: the handshake (ARC4 + Diffie-Hellman). Until it exists, a real client cannot get past"
Write-Host "      the key exchange, so the game client is not part of this check yet."
