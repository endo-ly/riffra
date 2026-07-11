[CmdletBinding()]
param(
    [ValidateSet('Debug', 'Release')]
    [string]$Configuration = 'Debug'
)

$ErrorActionPreference = 'Stop'
$ProcessPath = [System.Environment]::GetEnvironmentVariable('PATH', [System.EnvironmentVariableTarget]::Process)
[System.Environment]::SetEnvironmentVariable('PATH', $null, [System.EnvironmentVariableTarget]::Process)
[System.Environment]::SetEnvironmentVariable('Path', $ProcessPath, [System.EnvironmentVariableTarget]::Process)

$Root = Split-Path -Parent $PSScriptRoot
$CMake = 'C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe'
$Source = Join-Path $Root 'native\audio-engine'
$Build = Join-Path $Source 'build'

if (-not (Test-Path -LiteralPath $CMake)) {
    throw "Visual Studio CMake was not found at $CMake"
}

& $CMake -S $Source -B $Build -G 'Visual Studio 17 2022' -A x64
if ($LASTEXITCODE -ne 0) { throw 'Native audio engine configuration failed.' }

& $CMake --build $Build --config $Configuration --parallel 1
if ($LASTEXITCODE -ne 0) { throw 'Native audio engine build failed.' }

$Executable = Join-Path $Build "riffra-audio_artefacts\$Configuration\riffra-audio.exe"
$ScannerExecutable = Join-Path $Build "riffra-plugin-scan_artefacts\$Configuration\riffra-plugin-scan.exe"
if (-not (Test-Path -LiteralPath $Executable)) {
    throw "Native audio executable was not produced at $Executable"
}
if (-not (Test-Path -LiteralPath $ScannerExecutable)) {
    throw "Native plugin scanner was not produced at $ScannerExecutable"
}

$SidecarDirectory = Join-Path $Root 'src-tauri\binaries'
$Sidecar = Join-Path $SidecarDirectory 'riffra-audio-x86_64-pc-windows-msvc.exe'
$ScannerSidecar = Join-Path $SidecarDirectory 'riffra-plugin-scan-x86_64-pc-windows-msvc.exe'
New-Item -ItemType Directory -Path $SidecarDirectory -Force | Out-Null
Copy-Item -LiteralPath $Executable -Destination $Sidecar -Force
Copy-Item -LiteralPath $ScannerExecutable -Destination $ScannerSidecar -Force

Write-Output $Sidecar, $ScannerSidecar
