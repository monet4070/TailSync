param(
    [string]$WinRoot,
    [string]$MacRoot
)

$ErrorActionPreference = 'Stop'
if ($env:OS -eq 'Windows_NT') {
    $processPath = $env:Path
    [Environment]::SetEnvironmentVariable('PATH', $null, 'Process')
    [Environment]::SetEnvironmentVariable('Path', $processPath, 'Process')
}
$currentRoot = (Resolve-Path -LiteralPath (Split-Path $PSScriptRoot -Parent)).Path
$siblingRoot = Split-Path $currentRoot -Parent
$currentIsMac = (Test-Path -LiteralPath (Join-Path $currentRoot 'swift-ui')) -and
    (Test-Path -LiteralPath (Join-Path $currentRoot 'build-mac.sh'))
if (!$WinRoot) {
    $WinRoot = if ($currentIsMac) { Join-Path $siblingRoot 'tailsync-v2-win' } else { $currentRoot }
}
if (!$MacRoot) {
    $MacRoot = if ($currentIsMac) { $currentRoot } else { Join-Path $siblingRoot 'tailsync-v2-mac-1' }
}
$winRoot = (Resolve-Path -LiteralPath $WinRoot).Path
$macRoot = (Resolve-Path -LiteralPath $MacRoot).Path
if ([StringComparer]::OrdinalIgnoreCase.Equals($winRoot, $macRoot)) {
    throw 'Windows and macOS roots must be different directories.'
}
foreach ($manifest in @(
    (Join-Path $winRoot 'src-tauri\Cargo.toml'),
    (Join-Path $macRoot 'src-tauri\Cargo.toml')
)) {
    if (!(Test-Path -LiteralPath $manifest)) {
        throw "TailSync Cargo manifest missing: $manifest"
    }
}
$runRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("tailsync-interop-" + [guid]::NewGuid().ToString('N'))

function Invoke-ProbeDirection(
    [string]$ResponderProbe,
    [string]$InitiatorProbe,
    [string]$ResponderLabel,
    [string]$InitiatorLabel,
    [string]$DirectionName
) {
    $directionRoot = Join-Path $runRoot $DirectionName
    $serverData = Join-Path $directionRoot 'server-data'
    $clientData = Join-Path $directionRoot 'client-data'
    $serverOut = Join-Path $directionRoot 'server.out'
    $serverErr = Join-Path $directionRoot 'server.err'
    New-Item -ItemType Directory -Path $serverData,$clientData -Force | Out-Null
    $server = $null

    try {
        $env:TAILSYNC_DATA_DIR = $serverData
        $serverOptions = @{
            FilePath = $ResponderProbe
            ArgumentList = @('server', '127.0.0.1:0')
            RedirectStandardOutput = $serverOut
            RedirectStandardError = $serverErr
            PassThru = $true
        }
        if ($env:OS -eq 'Windows_NT') { $serverOptions.WindowStyle = 'Hidden' }
        $server = Start-Process @serverOptions

        $ready = $null
        for ($attempt = 0; $attempt -lt 100; $attempt++) {
            Start-Sleep -Milliseconds 50
            if (Test-Path -LiteralPath $serverOut) {
                $ready = Get-Content -LiteralPath $serverOut |
                    Select-String '^READY\s+(\S+)\s+(\S+)$' | Select-Object -First 1
                if ($ready) { break }
            }
            if ($server.HasExited) { break }
        }
        if (!$ready) {
            $errorText = Get-Content -LiteralPath $serverErr -Raw -ErrorAction SilentlyContinue
            throw "$ResponderLabel responder did not become ready: $errorText"
        }

        $address = $ready.Matches[0].Groups[1].Value
        $serverKey = $ready.Matches[0].Groups[2].Value
        $env:TAILSYNC_DATA_DIR = $clientData
        $clientOutput = & $InitiatorProbe client $address $serverKey 2>&1
        if (!$? -or $clientOutput -notcontains 'CLIENT_SYNC_OK' -or
            $clientOutput -notcontains 'CLIENT_PAIRING_OK') {
            throw "$InitiatorLabel initiator failed: $($clientOutput -join [Environment]::NewLine)"
        }
        if (!$server.WaitForExit(10000)) {
            $server.Kill()
            throw "$ResponderLabel responder timed out"
        }
        $server.WaitForExit()
        $server.Refresh()
        $serverOutput = Get-Content -LiteralPath $serverOut
        if (!$server.HasExited -or $serverOutput -notcontains 'SERVER_SYNC_OK' -or
            $serverOutput -notcontains 'SERVER_PAIRING_OK') {
            $serverError = Get-Content -LiteralPath $serverErr -Raw -ErrorAction SilentlyContinue
            throw "$ResponderLabel responder failed (stdout=$($serverOutput -join ' | ')): $serverError"
        }

        Write-Output "$InitiatorLabel initiator -> $ResponderLabel responder passed."
    }
    finally {
        if ($server -and !$server.HasExited) { $server.Kill() }
    }
}

try {
    cargo build --locked --quiet --manifest-path (Join-Path $macRoot 'src-tauri\Cargo.toml') --example interop_probe
    if (!$?) { throw 'macOS-project probe build failed' }
    cargo build --locked --quiet --manifest-path (Join-Path $winRoot 'src-tauri\Cargo.toml') --example interop_probe
    if (!$?) { throw 'Windows-project probe build failed' }

    $probeName = if ($env:OS -eq 'Windows_NT') { 'interop_probe.exe' } else { 'interop_probe' }
    $macProbe = Join-Path $macRoot "src-tauri\target\debug\examples\$probeName"
    $winProbe = Join-Path $winRoot "src-tauri\target\debug\examples\$probeName"
    Invoke-ProbeDirection $macProbe $winProbe 'macOS-project' 'Windows-project' 'win-to-mac'
    Invoke-ProbeDirection $winProbe $macProbe 'Windows-project' 'macOS-project' 'mac-to-win'

    Write-Output 'Bidirectional cross-project Noise, pairing, reliable events, and resumable file blocks passed.'
}
finally {
    Remove-Item Env:TAILSYNC_DATA_DIR -ErrorAction SilentlyContinue
    if (Test-Path $runRoot) { Remove-Item -LiteralPath $runRoot -Recurse -Force }
}
