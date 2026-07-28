[CmdletBinding()]
param(
    [switch]$Native
)

$ErrorActionPreference = 'Stop'
$Root = (Resolve-Path (Split-Path -Parent $PSScriptRoot)).Path
$ArtifactsRoot = Join-Path $Root '.artifacts\verify'
$env:CARGO_TARGET_DIR = Join-Path $ArtifactsRoot 'cargo'

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
    Invoke-Checked 'Regenerate TypeScript bindings' { npm run gen:types }
    Invoke-Checked 'TypeScript bindings staleness' { git diff --exit-code HEAD -- src/lib/generated }
    Invoke-Checked 'TypeScript build and tests' { npm run check }
    Invoke-Checked 'ESLint' { npm run lint }
    Invoke-Checked 'Prettier check' { npm run format:check }
    Invoke-Checked 'Knip (unused files and dependencies)' {
        npx knip --tsConfig tsconfig.app.json --include='files,dependencies' --no-config-hints
    }
    Invoke-Checked 'Rust formatting' { cargo fmt --manifest-path 'src-tauri\Cargo.toml' --check }
    Invoke-Checked 'Rust clippy' { cargo clippy --manifest-path 'src-tauri\Cargo.toml' --all-targets -- -D warnings }
    Invoke-Checked 'Rust tests' { cargo test --manifest-path 'src-tauri\Cargo.toml' }

    if ($Native) {
        Invoke-Checked 'Native sidecar build' {
            & '.\scripts\build-native.ps1' -Configuration Debug -WithTests
        }

        New-Item -ItemType Directory -Path $ArtifactsRoot -Force | Out-Null

        $CTest = 'C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\ctest.exe'
        if (-not (Test-Path -LiteralPath $CTest)) {
            throw "CTest was not found at $CTest"
        }
        Invoke-Checked 'Native C++ tests' {
            & $CTest --test-dir 'native\audio-engine\build' -C Debug --output-on-failure
        }
    }

    if (Get-Command clang-format -ErrorAction SilentlyContinue) {
        $CppFiles = Get-ChildItem -Path 'native\audio-engine' -Recurse -Include *.cpp,*.h,*.hpp,*.cc,*.hh |
            Select-Object -ExpandProperty FullName
        if ($CppFiles) {
            Invoke-Checked 'C++ formatting' { clang-format --dry-run --Werror @CppFiles }
        }
    }
    else {
        Write-Host "`n== C++ formatting skipped: clang-format is not installed ==" -ForegroundColor Yellow
    }

    Invoke-Checked 'Git whitespace check' { git diff --check }
    Write-Host "`nVerification completed successfully." -ForegroundColor Green
}
finally {
    Pop-Location
}
