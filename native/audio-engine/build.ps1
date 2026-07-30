# Build the native audio engine, run tests, and install sidecar binaries
# to apps/desktop/src-tauri/binaries.
param(
    [ValidateSet('Release', 'Debug')]
    [string] $Configuration = 'Release',

    [string] $BuildDirectory = (Join-Path $PSScriptRoot 'build'),

    [switch] $SkipTests
)

$ErrorActionPreference = 'Stop'
$engineDir = $PSScriptRoot
$repoRoot = Split-Path -Parent (Split-Path -Parent $engineDir)

function Find-Executable {
    param([string]$Name, [string]$Fallback)
    $fromPath = Get-Command $Name -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source
    if ($fromPath) { return $fromPath }
    if ($Fallback -and (Test-Path -LiteralPath $Fallback)) { return $Fallback }
    throw "$Name not found. Install CMake or add it to PATH."
}

$vsCMake = 'C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe'
$vsCtest = 'C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\ctest.exe'

$cmake = Find-Executable -Name 'cmake' -Fallback $vsCMake
$ctest = Find-Executable -Name 'ctest' -Fallback $vsCtest

$buildDir = $BuildDirectory

# Use the Visual Studio multi-config generator on Windows so that Debug and
# Release builds coexist under the same build tree. --config selects the actual
# configuration; -DCMAKE_BUILD_TYPE is ignored by multi-config generators.
& $cmake -S $engineDir -B $buildDir -G 'Visual Studio 17 2022' -A x64
if ($LASTEXITCODE -ne 0) { throw 'Native audio engine configuration failed.' }

& $cmake --build $buildDir --config $Configuration --parallel
if ($LASTEXITCODE -ne 0) { throw 'Native audio engine build failed.' }

if (-not $SkipTests) {
    & $ctest --test-dir $buildDir -C $Configuration --output-on-failure
    if ($LASTEXITCODE -ne 0) { throw 'Native audio engine tests failed.' }
}

& $cmake --install $buildDir --prefix $repoRoot --component riffra-sidecars --config $Configuration
if ($LASTEXITCODE -ne 0) { throw 'Native audio engine install failed.' }

Write-Host "Audio engine built, tested, and installed to apps/desktop/src-tauri/binaries" -ForegroundColor Green
