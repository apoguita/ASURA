<#
.SYNOPSIS
    Build an oracle: a small tool that compiles the reference's own code, unmodified, and runs it here.

.DESCRIPTION
    Two of them so far, both in `tools/`:

      * `path-oracle.c` calls `PathFinding`, so a port of the A* can be asked "what waypoints would the
        reference produce for this walk";
      * `pack-oracle.c` calls `pack_msg` with the reference's own format table, so a message's *bytes* can
        be produced by the reference rather than by my reading of its field list.

    Both `#include` the reference's single translation unit (`PreSearing/code/OpenTyria/
    build_open_tyria.c`) with `COMPILE_TOOLS`, exactly as the reference's own host tools do.

    Nothing under `PreSearing/` is written to. Its sources, its headers, the vendored mbedtls, and the
    static libraries and object files its own CMake build already produced are read; the tool lands here.

    Why MSVC and not gcc: the reference was built with Visual Studio 17 2022 (`build/CMakeCache.txt`), and
    an A*'s result is sensitive to floating-point code generation. Compiling with the toolchain the
    reference was built with removes one source of difference between the two sides.

    The libraries are the ones CMake built into `PreSearing/build/`; if they are missing, build the
    reference first (`cmake --build build --config RelWithDebInfo`), which this script never does itself.

.EXAMPLE
    .\tools\build-oracle.ps1

.EXAMPLE
    .\tools\build-oracle.ps1 -Source .\tools\pack-oracle.c -OutFile .\tools\pack-oracle.exe

.EXAMPLE
    .\tools\build-oracle.ps1 -Reference "D:\GWPS"
#>

param(
    [string] $Reference = (Join-Path (Split-Path -Parent (Split-Path -Parent $PSScriptRoot)) "PreSearing"),
    [string] $Source = (Join-Path $PSScriptRoot "path-oracle.c"),
    [string] $OutFile = ""
)

if (-not $OutFile) {
    $OutFile = [System.IO.Path]::ChangeExtension($Source, ".exe")
}
$OutFile = [System.IO.Path]::GetFullPath($OutFile)
$Source = [System.IO.Path]::GetFullPath($Source)

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

$includeDir = Join-Path $Reference "code\OpenTyria"
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
    "/I `"$includeDir`" /I `"$mbedtls`"",
    "`"$Source`"",
    "/Fe:`"$OutFile`" /Fo:`"$object`"",
    "/link `"$sqlite`" " + (($mbedLibs | ForEach-Object { "`"$_`"" }) -join " "),
    "Ws2_32.lib bcrypt.lib ntdll.lib"
) -join " "

Write-Host ("compiling {0} with the toolchain the reference was built with" -f (Split-Path -Leaf $Source))
cmd /c $command
if ($LASTEXITCODE -ne 0) {
    Write-Host "the compiler refused; nothing was written" -ForegroundColor Red
    exit 1
}

Write-Host ("built {0}" -f $OutFile) -ForegroundColor Green
exit 0
