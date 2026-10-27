<#
.SYNOPSIS
    Build the path oracle: the reference's own path finder, compiled here and run there.

.DESCRIPTION
    `tools/path-oracle.c` includes the reference's single translation unit unmodified
    (`PreSearing/code/OpenTyria/build_open_tyria.c`) and calls `PathFinding` directly, which is the only
    way to ask the reference "what waypoints would you produce for this walk" without a game client and
    without editing the reference.

    Nothing under `PreSearing/` is written to. Its sources, its headers, the vendored mbedtls, and the
    static libraries and object files its own CMake build already produced are read; the tool lands here.

    Why MSVC and not gcc: the reference was built with Visual Studio 17 2022 (`build/CMakeCache.txt`), and
    the search's result is sensitive to floating-point code generation. Compiling with the toolchain the
    reference was built with removes one source of difference between the two sides.

    The libraries are the ones CMake built into `PreSearing/build/`; if they are missing, build the
    reference first (`cmake --build build --config RelWithDebInfo`), which this script never does itself.

.EXAMPLE
    .\tools\build-path-oracle.ps1

.EXAMPLE
    .\tools\build-path-oracle.ps1 -Reference "D:\GWPS"
#>

param(
    [string] $Reference = (Join-Path (Split-Path -Parent (Split-Path -Parent $PSScriptRoot)) "PreSearing"),
    [string] $OutFile = (Join-Path $PSScriptRoot "path-oracle.exe")
)

$ErrorActionPreference = "Stop"

function Find-VcVars {
    $roots = @(
        "C:\Program Files\Microsoft Visual Studio\2022",
        "C:\Program Files (x86)\Microsoft Visual Studio\2022"
    )
    foreach ($root in $roots) {
        if (-not (Test-Path $root)) { continue }
        $found = Get-ChildItem $root -Directory -ErrorAction SilentlyContinue |
            ForEach-Object { Join-Path $_.FullName "VC\Auxiliary\Build\vcvars64.bat" } |
            Where-Object { Test-Path $_ } |
            Select-Object -First 1
        if ($found) { return $found }
    }
    return $null
}

$vcvars = Find-VcVars
if (-not $vcvars) {
    Write-Host "no Visual Studio 2022 with the C++ tools: install it, or build the oracle by hand" -ForegroundColor Yellow
    exit 2
}

$source = Join-Path $Reference "code\OpenTyria"
$mbedtls = Join-Path $Reference "deps\mbedtls-3.6.0\include"
$build = Join-Path $Reference "build"
$sqlite = Join-Path $build "RelWithDebInfo\gwps_sqlite.lib"
$mbedLibs = @(
    (Join-Path $build "mbedtls\library\RelWithDebInfo\mbedtls.lib"),
    (Join-Path $build "mbedtls\library\RelWithDebInfo\mbedx509.lib"),
    (Join-Path $build "mbedtls\library\RelWithDebInfo\mbedcrypto.lib"),
    (Join-Path $build "mbedtls\3rdparty\everest\RelWithDebInfo\everest.lib"),
    (Join-Path $build "mbedtls\3rdparty\p256-m\RelWithDebInfo\p256m.lib")
)

foreach ($needed in (@($sqlite) + $mbedLibs)) {
    if (-not (Test-Path $needed)) {
        Write-Host "missing $needed" -ForegroundColor Yellow
        Write-Host "build the reference first: cmake --build `"$build`" --config RelWithDebInfo" -ForegroundColor Yellow
        exit 2
    }
}

$object = [System.IO.Path]::ChangeExtension($OutFile, ".obj")
$command = @(
    # `&&` matters: without it cmd passes the rest of the line as arguments to vcvars64.bat, which
    # silences the compiler and reports only "the compiler refused".
    "call `"$vcvars`" >nul 2>&1 &&",
    "cl /nologo /O2 /MD /W0 /DCOMPILE_TOOLS",
    "/I `"$source`" /I `"$mbedtls`"",
    "`"$(Join-Path $PSScriptRoot 'path-oracle.c')`"",
    "/Fe:`"$OutFile`" /Fo:`"$object`"",
    "/link `"$sqlite`" " + (($mbedLibs | ForEach-Object { "`"$_`"" }) -join " "),
    "Ws2_32.lib bcrypt.lib ntdll.lib"
) -join " "

Write-Host "compiling the reference's path finder with the toolchain it was built with"
cmd /c $command
if ($LASTEXITCODE -ne 0) {
    Write-Host "the compiler refused; nothing was written" -ForegroundColor Red
    exit 1
}

Write-Host ("built {0}" -f $OutFile) -ForegroundColor Green
exit 0
