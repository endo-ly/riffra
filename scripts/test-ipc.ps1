[CmdletBinding()]
param(
    [string]$LoaderDll = $env:WEBVIEW2_LOADER_DLL,
    [switch]$CompileOnly
)

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
$manifest = Join-Path $repoRoot 'src-tauri/Cargo.toml'
if ([string]::IsNullOrWhiteSpace($LoaderDll)) {
    throw 'Set WEBVIEW2_LOADER_DLL or pass -LoaderDll with the matching WebView2Loader.dll path.'
}
$resolvedLoader = (Resolve-Path -LiteralPath $LoaderDll).Path
if ([IO.Path]::GetFileName($resolvedLoader) -ne 'WebView2Loader.dll') {
    throw "LoaderDll must point to WebView2Loader.dll: $resolvedLoader"
}

Write-Host 'Compiling the opt-in IPC integration tests.'
cargo test --manifest-path $manifest --features ipc-integration --lib --no-run

if ($CompileOnly) {
    Write-Host 'Compile-only check completed. No IPC test was executed.'
    exit 0
}

$deps = Join-Path $repoRoot 'src-tauri/target/debug/deps'
$binaries = Get-ChildItem -LiteralPath $deps -Filter 'riffra_lib-*.exe' -File
if ($binaries.Count -eq 0) {
    throw "No IPC test binary was produced in $deps"
}

foreach ($binary in $binaries) {
    Copy-Item -LiteralPath $resolvedLoader -Destination (Join-Path $binary.DirectoryName 'WebView2Loader.dll') -Force
}

Write-Host 'Running IPC integration tests with the explicitly supplied loader DLL.'
cargo test --manifest-path $manifest --features ipc-integration --lib
