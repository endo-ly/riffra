[CmdletBinding()]
param(
    [string]$LoaderDll = $env:WEBVIEW2_LOADER_DLL,
    [switch]$CompileOnly
)

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
$manifest = Join-Path $repoRoot 'apps/desktop/src-tauri/Cargo.toml'
if ([string]::IsNullOrWhiteSpace($LoaderDll)) {
    throw 'Set WEBVIEW2_LOADER_DLL or pass -LoaderDll with the matching WebView2Loader.dll path.'
}
$resolvedLoader = (Resolve-Path -LiteralPath $LoaderDll).Path
if ([IO.Path]::GetFileName($resolvedLoader) -ne 'WebView2Loader.dll') {
    throw "LoaderDll must point to WebView2Loader.dll: $resolvedLoader"
}

if ($CompileOnly) {
    Write-Host 'Compiling the opt-in IPC integration tests.'
    cargo test --manifest-path $manifest --features ipc-integration --lib --no-run
    Write-Host 'Compile-only check completed. No IPC test was executed.'
    exit 0
}

$deps = Join-Path $repoRoot 'target/debug/deps'
New-Item -ItemType Directory -Path $deps -Force | Out-Null
Copy-Item -LiteralPath $resolvedLoader -Destination (Join-Path $deps 'WebView2Loader.dll') -Force

Write-Host 'Running IPC integration tests with the explicitly supplied loader DLL.'
cargo test --manifest-path $manifest --features ipc-integration --lib
