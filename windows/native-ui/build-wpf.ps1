param(
    [ValidateSet('x64', 'ARM64')][string]$Architecture = 'x64',
    [ValidateSet('history', 'favorites', 'settings', 'connections')][string]$Page = 'history',
    [switch]$Run,
    [switch]$SmokeTest,
    [switch]$Publish,
    [switch]$Demo,
    [string]$BackendExecutable,
    [string]$ClipboardHelperExecutable,
    [string]$DotnetExecutable = 'dotnet',
    [string]$PublishDirectory,
    [string]$MigrationCheckDirectory
)
$ErrorActionPreference = 'Stop'
Push-Location $PSScriptRoot
try {
    & $DotnetExecutable run --project TailSync.Native.Checks -c Release
    if ($LASTEXITCODE -ne 0) { throw 'Shared presentation checks failed' }
    $runtime = if ($Architecture -eq 'ARM64') { 'win-arm64' } else { 'win-x64' }
    $canExecuteTarget = $Architecture -ne 'ARM64' -or [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString() -eq 'Arm64'
    $project = 'TailSync.Wpf/TailSync.Wpf.csproj'
    & $DotnetExecutable build $project -c Release -r $runtime --disable-build-servers
    if ($LASTEXITCODE -ne 0) { throw 'WPF build failed' }
    $binary = Join-Path $PSScriptRoot "TailSync.Wpf/bin/Release/net10.0-windows/$runtime/TailSync.Wpf.exe"
    if (-not $Demo -and (-not $SmokeTest -or $Run -or $Publish -or $MigrationCheckDirectory)) {
        if (-not $BackendExecutable) {
            $rustTarget = if ($Architecture -eq 'ARM64') { 'aarch64-pc-windows-msvc' } else { 'x86_64-pc-windows-msvc' }
            $manifest = Join-Path $PSScriptRoot '../src-tauri/Cargo.toml'
            cargo build --release --locked --features protocol-composite-clipboard --manifest-path $manifest --target $rustTarget
            if ($LASTEXITCODE -ne 0) { throw 'Rust backend build failed' }
            $metadata = cargo metadata --format-version 1 --no-deps --manifest-path $manifest | ConvertFrom-Json
            if ($LASTEXITCODE -ne 0) { throw 'Could not locate the Rust target directory' }
            $BackendExecutable = Join-Path $metadata.target_directory "$rustTarget/release/tailsync.exe"
            & (Join-Path $PSScriptRoot '../scripts/build-composite-helper.ps1') -Target $rustTarget
            if ($LASTEXITCODE -ne 0) { throw 'Windows clipboard helper build failed' }
            $ClipboardHelperExecutable = Join-Path $PSScriptRoot "../src-tauri/binaries/tailsync-clipboard-helper-$rustTarget.exe"
        }
        $BackendExecutable = (Resolve-Path -LiteralPath $BackendExecutable).Path
        # Verify the supplied cached backend matches the selected .NET runtime.
        $backendBytes = [IO.File]::ReadAllBytes($BackendExecutable)
        $peOffset = [BitConverter]::ToInt32($backendBytes, 0x3c)
        $machine = [BitConverter]::ToUInt16($backendBytes, $peOffset + 4)
        $expectedMachine = if ($Architecture -eq 'ARM64') { 0xaa64 } else { 0x8664 }
        if ($machine -ne $expectedMachine) { throw 'Backend architecture does not match the selected WPF architecture' }
        Copy-Item -LiteralPath $BackendExecutable -Destination (Join-Path (Split-Path $binary) 'TailSync.Backend.exe') -Force
        $clipboardHelper = if ($ClipboardHelperExecutable) { (Resolve-Path -LiteralPath $ClipboardHelperExecutable).Path } else { Join-Path (Split-Path $BackendExecutable) 'tailsync-clipboard-helper.exe' }
        if (-not (Test-Path -LiteralPath $clipboardHelper -PathType Leaf)) { throw 'The WPF composite backend requires its matching tailsync-clipboard-helper.exe. Supply -ClipboardHelperExecutable or build from source.' }
        $helperBytes = [IO.File]::ReadAllBytes($clipboardHelper)
        $helperPeOffset = [BitConverter]::ToInt32($helperBytes, 0x3c)
        if ([BitConverter]::ToUInt16($helperBytes, $helperPeOffset + 4) -ne $expectedMachine) { throw 'Clipboard helper architecture does not match the selected WPF architecture' }
        if ($canExecuteTarget) {
            $probe = & $clipboardHelper --self-check
            if ($LASTEXITCODE -ne 0 -or $probe -notmatch '^CWH1\s' -or $probe -notmatch '\bprotocol=2\b' -or $probe -notmatch '\banalyze=1\b') { throw 'Clipboard helper protocol/self-check failed' }
        }
        Copy-Item -LiteralPath $clipboardHelper -Destination (Join-Path (Split-Path $binary) 'tailsync-clipboard-helper.exe') -Force
    }
    if ($SmokeTest) {
        $report = Join-Path $PSScriptRoot "TailSync.Wpf/bin/Release/net10.0-windows/$runtime/wpf-smoke-results.json"
        $process = Start-Process -FilePath $binary -ArgumentList '--smoke-test', "--report=`"$report`"" -WorkingDirectory $PSScriptRoot -PassThru -Wait -WindowStyle Hidden
        if ($process.ExitCode -ne 0) { throw "WPF runtime smoke checks failed; see $report" }
        Get-Content $report
    }
    if ($Publish) {
        if (-not $PublishDirectory) { $PublishDirectory = Join-Path $PSScriptRoot "publish/wpf/$runtime" }
        & $DotnetExecutable publish $project -c Release -r $runtime --self-contained true -o $PublishDirectory --disable-build-servers
        if ($LASTEXITCODE -ne 0) { throw 'WPF publish failed' }
        if (-not $Demo) { Copy-Item -LiteralPath $BackendExecutable -Destination (Join-Path $PublishDirectory 'TailSync.Backend.exe') -Force }
        if ($clipboardHelper -and (Test-Path -LiteralPath $clipboardHelper)) { Copy-Item -LiteralPath $clipboardHelper -Destination (Join-Path $PublishDirectory 'tailsync-clipboard-helper.exe') -Force }
        if (-not $Demo -and $canExecuteTarget) {
            & (Join-Path $PublishDirectory 'TailSync.Backend.exe') --tailsync-clipboard-helper-check
            if ($LASTEXITCODE -ne 0) { throw 'Published WPF backend cannot resolve its matching clipboard helper' }
        }
        if (-not $canExecuteTarget) { Write-Output 'PE architecture checks passed; runtime helper checks require ARM64 Windows.' }
        Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'TailSync.Wpf/THIRD-PARTY-NOTICES.md') -Destination $PublishDirectory -Force
        Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'TailSync.Wpf/Licenses') -Destination $PublishDirectory -Recurse -Force
    }
    if ($MigrationCheckDirectory) {
        if ($Demo) { throw 'Migration checks require the real isolated backend' }
        $check = Start-Process -FilePath $binary -ArgumentList "--migration-check-dir=`"$MigrationCheckDirectory`"" -WorkingDirectory $PSScriptRoot -PassThru -Wait -WindowStyle Hidden
        if ($check.ExitCode -ne 0) { throw "WPF migration checks failed; see $MigrationCheckDirectory/migration.json" }
    }
    if ($Run) {
        $arguments = @("--window=$Page"); if ($Demo) { $arguments += '--demo' }
        Start-Process -FilePath $binary -ArgumentList $arguments -WorkingDirectory $PSScriptRoot -WindowStyle Normal
    }
} finally { Pop-Location }
