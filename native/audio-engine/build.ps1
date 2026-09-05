# Build the native audio engine, run tests, and install sidecars and resources.
param(
    [ValidateSet('Release', 'Debug')]
    [string] $Configuration = 'Release',

    [string] $BuildDirectory = (Join-Path $PSScriptRoot 'build'),

    [string] $Generator = 'Visual Studio 17 2022',

    [string] $Architecture = 'x64',

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
$headlessDestination = if ($env:RIFFRA_HEADLESS_BINARIES_DESTINATION) {
    $env:RIFFRA_HEADLESS_BINARIES_DESTINATION
} elseif ($Configuration -eq 'Debug') {
    'target/debug'
} else {
    'target/release'
}
$headlessResourcesDestination = if ($env:RIFFRA_HEADLESS_RESOURCES_DESTINATION) {
    $env:RIFFRA_HEADLESS_RESOURCES_DESTINATION
} elseif ($Configuration -eq 'Debug') {
    'target/debug'
} else {
    'target/release'
}
$sonalloyCargoProfile = if ($Configuration -eq 'Debug') { 'dev' } else { 'release' }

# Use the configured generator. Visual Studio generators support -A for the
# target architecture; other generators select their architecture themselves.
$configureArgs = @(
    '-S', $engineDir,
    '-B', $buildDir,
    '-G', $Generator,
    "-DRIFFRA_HEADLESS_BINARIES_DESTINATION=$headlessDestination",
    "-DRIFFRA_HEADLESS_RESOURCES_DESTINATION=$headlessResourcesDestination",
    "-DRIFFRA_SONALLOY_CARGO_PROFILE=$sonalloyCargoProfile"
)
if ($Generator -like 'Visual Studio *' -and $Architecture) {
    $configureArgs += @('-A', $Architecture)
}
& $cmake @configureArgs
if ($LASTEXITCODE -ne 0) { throw 'Native audio engine configuration failed.' }

& $cmake --build $buildDir --config $Configuration --parallel
if ($LASTEXITCODE -ne 0) { throw 'Native audio engine build failed.' }

if (-not $SkipTests) {
    & $ctest --test-dir $buildDir -C $Configuration --output-on-failure
    if ($LASTEXITCODE -ne 0) { throw 'Native audio engine tests failed.' }
}

& $cmake --install $buildDir --prefix $repoRoot --component riffra-sidecars --config $Configuration
if ($LASTEXITCODE -ne 0) { throw 'Native audio engine install failed.' }

Write-Host "Audio engine built, tested, and installed to apps/desktop/src-tauri and $headlessDestination" -ForegroundColor Green
