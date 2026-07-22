[CmdletBinding()]
param(
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$TauriArguments
)

$ErrorActionPreference = 'Stop'
$Root = (Resolve-Path (Split-Path -Parent $PSScriptRoot)).Path
$CargoTargetDirectory = Join-Path $Root '.artifacts\cargo-tauri-dev'

Push-Location $Root
try {
    $env:CARGO_TARGET_DIR = $CargoTargetDirectory

    Write-Host '== Native sidecar build ==' -ForegroundColor Cyan
    & '.\scripts\build-native.ps1' -Configuration Debug
    if ($LASTEXITCODE -ne 0) {
        throw "Native sidecar build failed with exit code $LASTEXITCODE."
    }

    Write-Host '== Tauri development server ==' -ForegroundColor Cyan
    & 'npm.cmd' run tauri dev -- @TauriArguments
    if ($LASTEXITCODE -ne 0) {
        throw "Tauri development server failed with exit code $LASTEXITCODE."
    }
}
finally {
    Pop-Location
}
