param(
    [Parameter(Mandatory = $true)]
    [string] $Destination
)

$ErrorActionPreference = 'Stop'

if ([string]::IsNullOrWhiteSpace($Destination)) {
    throw 'A destination for built-in instrument resources is required.'
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$version = if ($env:RIFFRA_SONALLOY_VERSION) {
    $env:RIFFRA_SONALLOY_VERSION
} else {
    $match = Select-String -Path (Join-Path $repoRoot 'native\audio-engine\CMakeLists.txt') `
        -Pattern '^set\(RIFFRA_SONALLOY_VERSION "([^"]*)"'
    if ($match) { $match.Matches[0].Groups[1].Value } else { '' }
}
if (-not $version) {
    throw 'Sonalloy release version could not be resolved from native/audio-engine/CMakeLists.txt'
}
$releaseTag = "v$version"

$temporaryBase = if ($env:RUNNER_TEMP) {
    $env:RUNNER_TEMP
} elseif ($env:TEMP) {
    $env:TEMP
} else {
    [System.IO.Path]::GetTempPath()
}
$temporaryRoot = Join-Path $temporaryBase ("riffra-sonalloy-resources-" + [guid]::NewGuid().ToString())

try {
    New-Item -ItemType Directory -Path $temporaryRoot | Out-Null
    git init -q $temporaryRoot
    if ($LASTEXITCODE -ne 0) { throw 'Could not initialize the temporary Sonalloy checkout.' }
    git -C $temporaryRoot remote add origin https://github.com/endo-ly/sonalloy.git
    if ($LASTEXITCODE -ne 0) { throw 'Could not configure the temporary Sonalloy checkout.' }
    git -C $temporaryRoot fetch --quiet --depth 1 origin $releaseTag
    if ($LASTEXITCODE -ne 0) { throw 'Could not fetch the pinned Sonalloy release.' }
    git -C $temporaryRoot checkout --quiet --detach FETCH_HEAD
    if ($LASTEXITCODE -ne 0) { throw 'Could not check out the pinned Sonalloy release.' }

    $stageScript = Join-Path $repoRoot 'native\audio-engine\cmake\stage_builtin_resources.cmake'
    & cmake `
        "-DSOURCE_PRESETS=$temporaryRoot\presets" `
        "-DDESTINATION=$Destination" `
        "-DSOURCE_RELEASE=$releaseTag" `
        -P $stageScript
    if ($LASTEXITCODE -ne 0) { throw 'Built-in instrument resource staging failed.' }
} finally {
    if (Test-Path -LiteralPath $temporaryRoot) {
        Remove-Item -LiteralPath $temporaryRoot -Recurse -Force
    }
}
