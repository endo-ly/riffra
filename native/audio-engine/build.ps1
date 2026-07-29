# Build the native audio engine, run tests, and install sidecar binaries
# to src-tauri/binaries.
param(
    [ValidateSet('Release', 'Debug')]
    [string] $Configuration = 'Release'
)

$ErrorActionPreference = 'Stop'
$engineDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot = Resolve-Path (Join-Path $engineDir '..\..')

function Find-Executable {
    param([string]$Name, [string]$Fallback)
    $fromPath = Get-Command $Name -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source
    if ($fromPath) { return $fromPath }
    if ($Fallback -and (Test-Path $Fallback)) { return $Fallback }
    Write-Error "$Name not found. Install CMake or add it to PATH."
}

$cmake = Find-Executable -Name 'cmake' -Fallback 'C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe'
$ctest = Find-Executable -Name 'ctest' -Fallback 'C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\ctest.exe'

Set-Location $engineDir
& $cmake -S . -B build -DCMAKE_BUILD_TYPE=$Configuration
& $cmake --build build --config $Configuration --parallel
& $cmake --build build --target riffra-plugin-scan --config $Configuration --parallel
& $ctest --test-dir build --output-on-failure -C $Configuration
& $cmake --install build --prefix $repoRoot --component riffra-sidecars

Write-Host "Audio engine built, tested, and installed to src-tauri/binaries" -ForegroundColor Green
