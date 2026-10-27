<#
.SYNOPSIS
    Seed the maps a world server can host, with the data a client needs to load them.

.DESCRIPTION
    Development convenience, not a deployment tool.

    A client entering a map is told three things that must all be right, or it never leaves the loading
    screen: which blob of Gw.dat to stream (`map_file_id`), and where inside it to put the character
    (`spawn_x`, `spawn_y`, `spawn_plane`). None of the three can be guessed -- (0,0) is outside the
    walkable geometry on every pre-Searing map measured so far.

    The values below are not invented. They come from the reference rig, where each map was verified
    against a live client on 2026-10-25: the tester stood a character in the map, read its position back
    (PreSearing/code/OpenTyria/GmMapsConfig.c), and confirmed it landed where it should. The blob id was
    confirmed separately, by matching the client's own terrain fingerprint against the archive entry.

    Idempotent: re-running updates the same rows.

    ASCII only, like every script in tools/: PowerShell 5.1 reads a BOM-less file as ANSI, where a UTF-8
    em dash breaks the parse far from the cause (docs/HANDOFF.md).

.EXAMPLE
    .\tools\seed-map.ps1

.EXAMPLE
    # after a world server refuses to start because a map is missing
    .\tools\seed-map.ps1
#>

$ErrorActionPreference = "Stop"

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

function Get-DatabaseUrl {
    $candidates = @(
        (Join-Path (Split-Path -Parent $PSScriptRoot) ".env"),
        (Join-Path $PSScriptRoot ".env")
    )
    foreach ($path in $candidates) {
        if (-not (Test-Path $path)) { continue }
        foreach ($line in Get-Content $path) {
            $trimmed = $line.Trim()
            if ($trimmed -eq "" -or $trimmed.StartsWith("#")) { continue }
            $parts = $trimmed.Split("=", 2)
            if ($parts.Count -eq 2 -and $parts[0].Trim() -eq "ASURA_DATABASE_URL") {
                $value = $parts[1].Trim().Trim("'").Trim('"')
                if ($value) { return $value }
            }
        }
    }
    throw "ASURA_DATABASE_URL not found. Copy .env.example to .env, or run tools/db-setup.ps1."
}

$script:Psql = Find-Psql
$url = Get-DatabaseUrl
Write-Host "psql: $script:Psql"

# map_file_id 0x1B97D (: 113021) is the pre-Searing region blob shared by Lakeside County, Ascalon City
# (pre-Searing) and Ashford Abbey. Sharing a blob is the normal case, not a bug: the map id plus the
# spawn point choose the place inside the region.
$sql = @"
INSERT INTO map_config (map_id, map_type, map_file_id, spawn_count, spawn_x, spawn_y, spawn_plane, name)
VALUES
    (146, 0, 113021, 1, 6602.0,    4485.0,    0, 'Lakeside County'),
    (148, 0, 113021, 1, 9541.873,  4796.456,  0, 'Ascalon City')
ON CONFLICT (map_id) DO UPDATE SET
    map_type    = EXCLUDED.map_type,
    map_file_id = EXCLUDED.map_file_id,
    spawn_count = EXCLUDED.spawn_count,
    spawn_x     = EXCLUDED.spawn_x,
    spawn_y     = EXCLUDED.spawn_y,
    name        = EXCLUDED.name,
    spawn_plane = EXCLUDED.spawn_plane;
"@

# psql's stderr is not fatal here: PowerShell turns native stderr into error records, and with
# $ErrorActionPreference = "Stop" a NOTICE would abort the seeding. Judge by the exit code instead.
$previous = $ErrorActionPreference
$ErrorActionPreference = "Continue"
try {
    $output = & $script:Psql -v ON_ERROR_STOP=1 -X -q -c $sql $url 2>&1 | ForEach-Object { "$_" }
    $exitCode = $LASTEXITCODE
} finally {
    $ErrorActionPreference = $previous
}
if ($exitCode -ne 0) {
    throw "psql failed (exit $exitCode) while seeding the maps:`n$($output -join "`n")"
}

Write-Host ""
Write-Host "maps seeded. A world server can host:"
Write-Host "  146  Lakeside County (outpost)   blob 113021  spawn 6602.0, 4485.0 plane 0"
Write-Host "  148  Ascalon City   (outpost)   blob 113021  spawn 9541.873, 4796.456 plane 0"
Write-Host ""
Write-Host "verify with: cargo run -p asura-world -- --maps 146 --world 8001"
