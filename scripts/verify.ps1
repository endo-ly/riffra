[CmdletBinding()]
param(
    [switch]$Native
)

$ErrorActionPreference = 'Stop'
$Root = (Resolve-Path (Split-Path -Parent $PSScriptRoot)).Path
$ArtifactsRoot = Join-Path $Root '.artifacts\verify'

function Invoke-Checked {
    param(
        [Parameter(Mandatory)]
        [string]$Name,
        [Parameter(Mandatory)]
        [scriptblock]$Command
    )

    Write-Host "`n== $Name ==" -ForegroundColor Cyan
    & $Command
    if ($LASTEXITCODE -ne 0) {
        throw "$Name failed with exit code $LASTEXITCODE."
    }
}

Push-Location $Root
try {
    Invoke-Checked 'TypeScript build and tests' { npm run check }
    Invoke-Checked 'Rust tests' { cargo test --manifest-path 'src-tauri\Cargo.toml' }

    if ($Native) {
        Invoke-Checked 'Native sidecar build' { & '.\scripts\build-native.ps1' -Configuration Debug }

        $RecordingTestDirectory = Join-Path $ArtifactsRoot 'recording-self-test'
        $ResolvedArtifactsRoot = [System.IO.Path]::GetFullPath($ArtifactsRoot)
        $ResolvedRecordingTestDirectory = [System.IO.Path]::GetFullPath($RecordingTestDirectory)
        if (-not $ResolvedRecordingTestDirectory.StartsWith($ResolvedArtifactsRoot, [System.StringComparison]::OrdinalIgnoreCase)) {
            throw 'The recording self-test directory escaped the repository artifact directory.'
        }
        if (Test-Path -LiteralPath $ResolvedRecordingTestDirectory) {
            Remove-Item -LiteralPath $ResolvedRecordingTestDirectory -Recurse -Force
        }
        New-Item -ItemType Directory -Path $ArtifactsRoot -Force | Out-Null

        $AudioSidecar = Join-Path $Root 'src-tauri\binaries\riffra-audio-x86_64-pc-windows-msvc.exe'
        Invoke-Checked 'Native recording self-test' {
            & $AudioSidecar --recording-self-test $ResolvedRecordingTestDirectory
        }
    }

    Invoke-Checked 'Git whitespace check' { git diff --check }
    Write-Host "`nVerification completed successfully." -ForegroundColor Green
}
finally {
    Pop-Location
}
