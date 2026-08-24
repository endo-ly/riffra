$ErrorActionPreference = 'Stop'

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
Set-Location $repoRoot

$dataRoot = Join-Path $env:RUNNER_TEMP ("riffra-headless-host-" + [guid]::NewGuid().ToString())
$stdoutLog = Join-Path $dataRoot 'serve.stdout.log'
$stderrLog = Join-Path $dataRoot 'serve.stderr.log'
$safeMode = if ($env:RIFFRA_HEADLESS_SAFE_MODE) { $env:RIFFRA_HEADLESS_SAFE_MODE } else { '1' }
if ($safeMode -notin @('0', '1')) { throw 'RIFFRA_HEADLESS_SAFE_MODE must be 0 or 1' }
New-Item -ItemType Directory -Path $dataRoot | Out-Null
$process = $null

try {
    cargo build -p riffra-cli
    $targetDirectory = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { 'target' }
    $binary = Join-Path $targetDirectory 'debug\riffra.exe'
    $serveArguments = @('--data-root', $dataRoot, 'serve')
    if ($safeMode -eq '1') { $serveArguments += '--safe-mode' }
    $process = Start-Process -FilePath $binary `
        -ArgumentList $serveArguments `
        -RedirectStandardOutput $stdoutLog `
        -RedirectStandardError $stderrLog `
        -PassThru

    $endpoint = Join-Path $dataRoot 'control\host.json'
    for ($i = 0; $i -lt 200; $i++) {
        if (Test-Path $endpoint) { break }
        if ($process.HasExited) {
            if (Test-Path $stderrLog) { Get-Content $stderrLog }
            throw 'riffra serve exited before publishing its endpoint'
        }
        Start-Sleep -Milliseconds 50
    }
    if (-not (Test-Path $endpoint)) { throw 'riffra serve did not publish its endpoint' }

    & $binary --data-root $dataRoot --attach session get |
        Out-File (Join-Path $dataRoot 'session.json')
    if ($LASTEXITCODE -ne 0) { throw 'Attached session.get failed' }
    $session = Get-Content (Join-Path $dataRoot 'session.json') | ConvertFrom-Json
    if (-not $session.ok -or $session.result.type -ne 'session' -or $session.sequence -ne 0) {
        throw 'Attached session.get returned an invalid contract'
    }
    & $binary --data-root $dataRoot --attach track add --name 'Process Test' --kind instrument |
        Out-File (Join-Path $dataRoot 'track.json')
    if ($LASTEXITCODE -ne 0) { throw 'Attached track.add failed' }
    $track = Get-Content (Join-Path $dataRoot 'track.json') | ConvertFrom-Json
    if (-not $track.ok -or $track.result.type -ne 'session' -or $track.sequence -ne 1) {
        throw 'Attached track.add returned an invalid contract'
    }
    & $binary --data-root $dataRoot --attach undo |
        Out-File (Join-Path $dataRoot 'undo.json')
    if ($LASTEXITCODE -ne 0) { throw 'Attached undo failed' }
    $undo = Get-Content (Join-Path $dataRoot 'undo.json') | ConvertFrom-Json
    if (-not $undo.ok -or $undo.result.type -ne 'arrangementMutation' -or $undo.sequence -ne 2) {
        throw 'Attached undo returned an invalid contract'
    }

    if ($safeMode -eq '1') {
        $transportOutput = & $binary --data-root $dataRoot --attach transport play --transport-sequence 1 2>&1
        $transportExitCode = $LASTEXITCODE
        if ($transportExitCode -eq 0) { throw 'transport play unexpectedly succeeded in Safe Mode' }
        if (-not (($transportOutput -join "`n") -match 'runtimeUnavailable')) {
            throw 'Safe Mode transport play did not return runtimeUnavailable'
        }

        $probeOutput = & $binary --data-root $dataRoot --attach audio probe 2>&1
        $probeExitCode = $LASTEXITCODE
        if ($probeExitCode -eq 0) { throw 'audio probe unexpectedly succeeded in Safe Mode' }
        if (-not (($probeOutput -join "`n") -match 'runtimeUnavailable')) {
            throw 'Safe Mode audio probe did not return runtimeUnavailable'
        }

        $pluginOutput = & $binary --data-root $dataRoot --attach plugin scan --path $dataRoot 2>&1
        $pluginExitCode = $LASTEXITCODE
        if ($pluginExitCode -eq 0) { throw 'plugin scan unexpectedly succeeded in Safe Mode' }
        if (-not (($pluginOutput -join "`n") -match 'runtimeUnavailable')) {
            throw 'Safe Mode plugin scan did not return runtimeUnavailable'
        }
    } else {
        & $binary --data-root $dataRoot --attach host status |
            Out-File (Join-Path $dataRoot 'host.json')
        if ($LASTEXITCODE -ne 0) { throw 'Attached host.status failed' }
        $hostStatus = Get-Content (Join-Path $dataRoot 'host.json') | ConvertFrom-Json
        if (-not $hostStatus.ok -or $hostStatus.result.type -ne 'hostStatus') {
            throw 'Attached host.status returned an invalid contract'
        }
        & $binary --data-root $dataRoot --attach audio status |
            Out-File (Join-Path $dataRoot 'audio.json')
        if ($LASTEXITCODE -ne 0) { throw 'Attached audio.status failed' }
        $audioStatus = Get-Content (Join-Path $dataRoot 'audio.json') | ConvertFrom-Json
        if (-not $audioStatus.ok -or $audioStatus.result.type -ne 'audioStatus') {
            throw 'Attached audio.status returned an invalid contract'
        }
    }

    & $binary --data-root $dataRoot --attach host shutdown |
        Out-File (Join-Path $dataRoot 'shutdown.json')
    if ($LASTEXITCODE -ne 0) { throw 'Attached host.shutdown failed' }
    $shutdown = Get-Content (Join-Path $dataRoot 'shutdown.json') | ConvertFrom-Json
    if (-not $shutdown.ok -or $shutdown.result.type -ne 'ok') {
        throw 'Attached host.shutdown returned an invalid contract'
    }
    for ($i = 0; $i -lt 200; $i++) {
        if ($process.HasExited) { break }
        Start-Sleep -Milliseconds 50
    }
    if (-not $process.HasExited) { throw 'riffra serve did not stop after host shutdown' }

    if (Test-Path $endpoint) { throw 'Host endpoint was not removed after shutdown' }
    $socket = Join-Path $dataRoot 'control\host.sock'
    if (Test-Path $socket) { throw 'Unix socket artifact was not removed after shutdown' }
    & $binary --data-root $dataRoot session get |
        Out-File (Join-Path $dataRoot 'reopened.json')
    if ($LASTEXITCODE -ne 0) { throw 'Standalone reopen failed after host shutdown' }
    $reopened = Get-Content (Join-Path $dataRoot 'reopened.json') | ConvertFrom-Json
    if (-not $reopened.ok -or $reopened.result.type -ne 'session' -or $reopened.sequence -ne 0) {
        throw 'Standalone reopen returned an invalid contract'
    }
}
finally {
    if ($null -ne $process -and -not $process.HasExited) {
        Stop-Process -Id $process.Id -Force
        $process.WaitForExit()
    }
    if (Test-Path $dataRoot) { Remove-Item -LiteralPath $dataRoot -Recurse -Force }
}
