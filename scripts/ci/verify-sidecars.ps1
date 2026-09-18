[CmdletBinding()]
param(
    [ValidateSet('Debug', 'RelWithDebInfo', 'Release')]
    [string]$Configuration = 'Debug'
)

$ErrorActionPreference = 'Stop'
$targetDirectory = if ($Configuration -eq 'Release') { 'target/release' } else { 'target/debug' }
$targetTriple = (rustc -vV | Select-String '^host: ').ToString().Split(':', 2)[1].Trim()

foreach ($sidecar in @('riffra-audio', 'riffra-plugin-scan', 'riffra-render', 'sonalloy')) {
    $headless = Join-Path $targetDirectory "$sidecar.exe"
    $packaged = Join-Path 'apps/desktop/src-tauri/binaries' "$sidecar-$targetTriple.exe"
    if (!(Test-Path -LiteralPath $headless -PathType Leaf)) { throw "$headless is missing" }
    if (!(Test-Path -LiteralPath $packaged -PathType Leaf)) { throw "$packaged is missing" }
}
