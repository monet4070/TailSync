[CmdletBinding()]
param(
    [string]$Target = 'x86_64-pc-windows-msvc',
    [string]$OutputDirectory = 'release',
    [string]$BuildDirectory = 'target-package',
    [string]$CertificateThumbprint = $env:TAILSYNC_WINDOWS_CERTIFICATE_THUMBPRINT,
    [string]$TimestampUrl = 'https://timestamp.digicert.com',
    [switch]$InstallDependencies,
    [switch]$SkipChecks,
    [switch]$SkipSmokeTest,
    [switch]$Release,
    [ValidateSet('community', 'trusted')]
    [string]$ReleaseTier = 'community'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Invoke-Checked {
    param(
        [Parameter(Mandatory)] [string]$FilePath,
        [Parameter()] [string[]]$Arguments = @()
    )

    & $FilePath @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "Command failed with exit code ${LASTEXITCODE}: $FilePath $($Arguments -join ' ')"
    }
}

function Resolve-OutputPath {
    param(
        [Parameter(Mandatory)] [string]$BasePath,
        [Parameter(Mandatory)] [string]$RequestedPath
    )

    if ([System.IO.Path]::IsPathRooted($RequestedPath)) {
        return [System.IO.Path]::GetFullPath($RequestedPath)
    }
    return [System.IO.Path]::GetFullPath((Join-Path $BasePath $RequestedPath))
}

function New-ArtifactRecord {
    param(
        [Parameter(Mandatory)] [string]$Kind,
        [Parameter(Mandatory)] [System.IO.FileInfo]$File
    )

    $signature = Get-AuthenticodeSignature -LiteralPath $File.FullName
    [ordered]@{
        kind = $Kind
        file = $File.Name
        bytes = $File.Length
        sha256 = (Get-FileHash -LiteralPath $File.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
        signature = $signature.Status.ToString()
    }
}

if ($env:OS -ne 'Windows_NT') {
    throw 'This packaging script must run on Windows.'
}

if ($Release) {
    foreach ($requiredValue in @(
        @{ Name = 'TAILSYNC_UPDATER_PUBLIC_KEY'; Value = $env:TAILSYNC_UPDATER_PUBLIC_KEY },
        @{ Name = 'TAURI_SIGNING_PRIVATE_KEY'; Value = $env:TAURI_SIGNING_PRIVATE_KEY }
    )) {
        if ([string]::IsNullOrWhiteSpace([string]$requiredValue.Value)) {
            throw "$($requiredValue.Name) is required for every published release."
        }
    }
    if ($ReleaseTier -eq 'trusted' -and [string]::IsNullOrWhiteSpace($CertificateThumbprint)) {
        throw 'TAILSYNC_WINDOWS_CERTIFICATE_THUMBPRINT is required for a trusted release.'
    }
}

$scriptRoot = Split-Path -Parent $PSCommandPath
$windowsRoot = (Resolve-Path -LiteralPath (Join-Path $scriptRoot '..')).Path
$repositoryRoot = (Resolve-Path -LiteralPath (Join-Path $windowsRoot '..')).Path
$tauriRoot = Join-Path $windowsRoot 'src-tauri'
$manifestPath = Join-Path $tauriRoot 'Cargo.toml'
$configPath = Join-Path $tauriRoot 'tauri.conf.json'
$lockPath = Join-Path $tauriRoot 'Cargo.lock'
$sharedManifest = Join-Path $repositoryRoot 'shared\rust-core\Cargo.toml'
$tauriCli = Join-Path $windowsRoot 'node_modules\.bin\tauri.cmd'
$targetDirectory = Resolve-OutputPath -BasePath $tauriRoot -RequestedPath $BuildDirectory
$releaseDirectory = Resolve-OutputPath -BasePath $windowsRoot -RequestedPath $OutputDirectory

$requiredFiles = @(
    $manifestPath,
    $configPath,
    $lockPath,
    $sharedManifest,
    (Join-Path $windowsRoot 'package-lock.json'),
    (Join-Path $windowsRoot 'history.html'),
    (Join-Path $windowsRoot 'settings.html'),
    (Join-Path $windowsRoot 'src\history-main.tsx'),
    (Join-Path $windowsRoot 'src\settings-main.tsx')
)
foreach ($requiredFile in $requiredFiles) {
    if (!(Test-Path -LiteralPath $requiredFile -PathType Leaf)) {
        throw "Required packaging input is missing: $requiredFile"
    }
}

foreach ($command in @('node', 'npm', 'cargo', 'rustc')) {
    if (!(Get-Command $command -ErrorAction SilentlyContinue)) {
        throw "Required command is not available on PATH: $command"
    }
}

Push-Location $windowsRoot
try {
    if ($InstallDependencies -or !(Test-Path -LiteralPath $tauriCli -PathType Leaf)) {
        Write-Host 'Installing locked frontend dependencies...'
        Invoke-Checked -FilePath 'npm' -Arguments @('ci')
    }
    if (!(Test-Path -LiteralPath $tauriCli -PathType Leaf)) {
        throw "Local Tauri CLI is missing after npm ci: $tauriCli"
    }

    $config = Get-Content -Raw -LiteralPath $configPath | ConvertFrom-Json
    $version = [string]$config.version
    $productName = [string]$config.productName
    if ([string]::IsNullOrWhiteSpace($version) -or [string]::IsNullOrWhiteSpace($productName)) {
        throw 'tauri.conf.json must define productName and version.'
    }
    if ($config.build.frontendDist -ne '../dist') {
        throw "Unexpected frontendDist for the current layout: $($config.build.frontendDist)"
    }

    Write-Host "Packaging $productName $version for $Target"
    Invoke-Checked -FilePath 'cargo' -Arguments @(
        'metadata', '--locked', '--no-deps', '--format-version', '1',
        '--manifest-path', $manifestPath
    )

    if (!$SkipChecks) {
        Write-Host 'Checking generated contracts...'
        Invoke-Checked -FilePath 'node' -Arguments @(
            (Join-Path $repositoryRoot 'shared\schema\generate-settings.mjs'), '--check'
        )

        Write-Host 'Checking cross-platform contracts...'
        & (Join-Path $scriptRoot 'check_cross_platform_sync.ps1') `
            -WinRoot $windowsRoot `
            -MacRoot (Join-Path $repositoryRoot 'macos')

        Write-Host 'Running frontend lint and tests...'
        Invoke-Checked -FilePath 'npm' -Arguments @('run', 'lint')
        Invoke-Checked -FilePath 'npm' -Arguments @('test')

        Write-Host 'Running Windows Rust tests...'
        Invoke-Checked -FilePath 'cargo' -Arguments @(
            'test', '--locked', '--manifest-path', $manifestPath, '--lib'
        )
    }

    New-Item -ItemType Directory -Path $releaseDirectory -Force | Out-Null
    New-Item -ItemType Directory -Path $targetDirectory -Force | Out-Null

    $previousTargetDirectory = $env:CARGO_TARGET_DIR
    $previousCi = $env:CI
    $previousPublishedRelease = $env:TAILSYNC_PUBLISHED_RELEASE
    try {
        $env:CARGO_TARGET_DIR = $targetDirectory
        $env:CI = 'true'
        if ($Release) {
            $env:TAILSYNC_PUBLISHED_RELEASE = '1'
        }
        Write-Host 'Building the release binary and NSIS installer...'
        $buildArguments = @('build', '--target', $Target, '--bundles', 'nsis', '--ci')
        if ($Release -and $ReleaseTier -eq 'trusted') {
            $releaseConfig = [ordered]@{
                bundle = [ordered]@{
                    windows = [ordered]@{
                        certificateThumbprint = $CertificateThumbprint
                        digestAlgorithm = 'sha256'
                        timestampUrl = $TimestampUrl
                    }
                }
            } | ConvertTo-Json -Depth 5 -Compress
            $buildArguments += @('--config', $releaseConfig)
        } else {
            $buildArguments += '--no-sign'
        }
        Invoke-Checked -FilePath $tauriCli -Arguments $buildArguments
    }
    finally {
        if ($null -eq $previousTargetDirectory) {
            Remove-Item Env:CARGO_TARGET_DIR -ErrorAction SilentlyContinue
        } else {
            $env:CARGO_TARGET_DIR = $previousTargetDirectory
        }
        if ($null -eq $previousCi) {
            Remove-Item Env:CI -ErrorAction SilentlyContinue
        } else {
            $env:CI = $previousCi
        }
        if ($null -eq $previousPublishedRelease) {
            Remove-Item Env:TAILSYNC_PUBLISHED_RELEASE -ErrorAction SilentlyContinue
        } else {
            $env:TAILSYNC_PUBLISHED_RELEASE = $previousPublishedRelease
        }
    }

    $targetRelease = Join-Path $targetDirectory "$Target\release"
    $portableSource = Join-Path $targetRelease 'tailsync.exe'
    if (!(Test-Path -LiteralPath $portableSource -PathType Leaf)) {
        throw "Tauri did not produce the release executable: $portableSource"
    }
    $installerSource = Get-ChildItem -LiteralPath (Join-Path $targetRelease 'bundle\nsis') `
        -Filter '*.exe' -File |
        Sort-Object LastWriteTimeUtc -Descending |
        Select-Object -First 1
    if ($null -eq $installerSource) {
        throw 'Tauri did not produce an NSIS installer.'
    }

    $architecture = if ($Target.StartsWith('aarch64-')) { 'arm64' } else { 'x64' }
    $portablePath = Join-Path $releaseDirectory "$productName-$version-Windows-$architecture-portable.exe"
    $installerPath = Join-Path $releaseDirectory "$productName-$version-Windows-$architecture-setup.exe"
    Copy-Item -LiteralPath $portableSource -Destination $portablePath -Force
    Copy-Item -LiteralPath $installerSource.FullName -Destination $installerPath -Force

    $portableFile = Get-Item -LiteralPath $portablePath
    $installerFile = Get-Item -LiteralPath $installerPath
    if ($Release -and $ReleaseTier -eq 'trusted') {
        foreach ($signedFile in @($portableFile, $installerFile)) {
            $signature = Get-AuthenticodeSignature -LiteralPath $signedFile.FullName
            if ($signature.Status -ne [System.Management.Automation.SignatureStatus]::Valid) {
                throw "Trusted release artifact is not Authenticode-valid: $($signedFile.Name) ($($signature.Status))"
            }
        }
    }
    $artifacts = @(
        (New-ArtifactRecord -Kind 'portable' -File $portableFile),
        (New-ArtifactRecord -Kind 'nsis-installer' -File $installerFile)
    )
    $updaterPath = $null
    $updaterSignaturePath = $null
    if ($Release) {
        Write-Host 'Building and signing the downgrade-resistant updater archive...'
        $updaterPath = Join-Path $releaseDirectory "$productName-$version-Windows-$architecture.nsis.zip"
        $updaterSignaturePath = "$updaterPath.sig"
        $updaterStaging = Join-Path ([System.IO.Path]::GetTempPath()) `
            ('tailsync-updater-' + [guid]::NewGuid().ToString('N'))
        New-Item -ItemType Directory -Path $updaterStaging | Out-Null
        try {
            Copy-Item -LiteralPath $installerPath -Destination (Join-Path $updaterStaging $installerFile.Name)
            $metadata = [ordered]@{
                schema = 1
                product = $productName
                version = $version
            } | ConvertTo-Json -Compress
            $metadataPath = Join-Path $updaterStaging 'tailsync-update.json'
            [System.IO.File]::WriteAllText(
                $metadataPath,
                $metadata,
                [System.Text.UTF8Encoding]::new($false)
            )
            Remove-Item -LiteralPath $updaterPath, $updaterSignaturePath -Force -ErrorAction SilentlyContinue
            Compress-Archive -LiteralPath @(
                (Join-Path $updaterStaging $installerFile.Name),
                $metadataPath
            ) -DestinationPath $updaterPath -CompressionLevel Optimal
            Invoke-Checked -FilePath $tauriCli -Arguments @('signer', 'sign', $updaterPath)
            if (!(Test-Path -LiteralPath $updaterSignaturePath -PathType Leaf)) {
                throw "Tauri signer did not produce $updaterSignaturePath"
            }
        }
        finally {
            $resolvedStaging = (Resolve-Path -LiteralPath $updaterStaging).Path
            $systemTemp = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
            if (!$resolvedStaging.StartsWith($systemTemp, [StringComparison]::OrdinalIgnoreCase)) {
                throw "Refusing to remove unexpected updater staging directory: $resolvedStaging"
            }
            Remove-Item -LiteralPath $resolvedStaging -Recurse -Force
        }
        $updaterFile = Get-Item -LiteralPath $updaterPath
        $updaterSignatureFile = Get-Item -LiteralPath $updaterSignaturePath
        $artifacts += [ordered]@{
            kind = 'updater'
            file = $updaterFile.Name
            bytes = $updaterFile.Length
            sha256 = (Get-FileHash -LiteralPath $updaterFile.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
            signature = $updaterSignatureFile.Name
        }
        $artifacts += [ordered]@{
            kind = 'updater-signature'
            file = $updaterSignatureFile.Name
            bytes = $updaterSignatureFile.Length
            sha256 = (Get-FileHash -LiteralPath $updaterSignatureFile.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
            signature = $null
        }
        $updaterPlatform = if ($Target.StartsWith('aarch64-')) {
            'windows-aarch64'
        } else {
            'windows-x86_64'
        }
        $releaseFragment = [ordered]@{
            schema = 1
            product = $productName
            version = $version
            platform = $updaterPlatform
            artifact = $updaterFile.Name
            signatureFile = $updaterSignatureFile.Name
        }
        $releaseFragmentPath = Join-Path $releaseDirectory "release-$updaterPlatform.json"
        $releaseFragment | ConvertTo-Json -Depth 3 | Set-Content `
            -LiteralPath $releaseFragmentPath -Encoding utf8
    }
    $checksumPath = Join-Path $releaseDirectory "$productName-$version-Windows-$architecture.sha256"
    $checksumLines = $artifacts | ForEach-Object { "$($_.sha256) *$($_.file)" }
    Set-Content -LiteralPath $checksumPath -Value $checksumLines -Encoding ascii

    $commit = (& git -C $repositoryRoot rev-parse --short HEAD 2>$null)
    if ($LASTEXITCODE -ne 0) { $commit = $null }
    $dirty = $null -ne (& git -C $repositoryRoot status --porcelain 2>$null | Select-Object -First 1)
    $manifest = [ordered]@{
        product = $productName
        version = $version
        target = $Target
        releaseTier = if ($Release) { $ReleaseTier } else { 'development' }
        builtAtUtc = [DateTime]::UtcNow.ToString('o')
        sourceCommit = $commit
        sourceDirty = $dirty
        rustc = (& rustc --version)
        node = (& node --version)
        tauri = (& $tauriCli --version)
        artifacts = $artifacts
    }
    $buildManifestPath = Join-Path $releaseDirectory "$productName-$version-Windows-$architecture-build.json"
    $manifest | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $buildManifestPath -Encoding utf8

    if (!$SkipSmokeTest) {
        Write-Host 'Running an isolated portable executable smoke test...'
        $smokeRoot = Join-Path ([System.IO.Path]::GetTempPath()) `
            ('tailsync-package-smoke-' + [guid]::NewGuid().ToString('N'))
        New-Item -ItemType Directory -Path $smokeRoot | Out-Null
        $previousDataDirectory = $env:TAILSYNC_DATA_DIR
        $process = $null
        try {
            $env:TAILSYNC_DATA_DIR = $smokeRoot
            $process = Start-Process -FilePath $portablePath -WindowStyle Hidden -PassThru
            Start-Sleep -Seconds 4
            $process.Refresh()
            if ($process.HasExited) {
                throw "Packaged executable exited during smoke test with code $($process.ExitCode)."
            }
        }
        finally {
            if ($null -ne $process -and !$process.HasExited) {
                Stop-Process -Id $process.Id -Force
                Wait-Process -Id $process.Id -Timeout 10 -ErrorAction SilentlyContinue
            }
            if ($null -eq $previousDataDirectory) {
                Remove-Item Env:TAILSYNC_DATA_DIR -ErrorAction SilentlyContinue
            } else {
                $env:TAILSYNC_DATA_DIR = $previousDataDirectory
            }
            $resolvedSmokeRoot = (Resolve-Path -LiteralPath $smokeRoot).Path
            $systemTemp = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
            if (!$resolvedSmokeRoot.StartsWith($systemTemp, [StringComparison]::OrdinalIgnoreCase)) {
                throw "Refusing to remove unexpected smoke-test directory: $resolvedSmokeRoot"
            }
            for ($attempt = 1; $attempt -le 5; $attempt++) {
                try {
                    Remove-Item -LiteralPath $resolvedSmokeRoot -Recurse -Force -ErrorAction Stop
                    break
                }
                catch {
                    if ($attempt -eq 5) { throw }
                    Start-Sleep -Milliseconds (200 * $attempt)
                }
            }
        }
    }

    Write-Host ''
    Write-Host 'Windows package completed:'
    Write-Host "  Portable:  $portablePath"
    Write-Host "  Installer: $installerPath"
    if ($Release) {
        Write-Host "  Updater:   $updaterPath"
        Write-Host "  Signature: $updaterSignaturePath"
    }
    Write-Host "  Checksums: $checksumPath"
    Write-Host "  Manifest:  $buildManifestPath"
    if ($Release -and $ReleaseTier -eq 'community') {
        Write-Host '  Platform trust: Community build (no Authenticode certificate)'
    } elseif ($Release -and $ReleaseTier -eq 'trusted') {
        Write-Host '  Platform trust: Trusted build (Authenticode verified)'
    }
}
finally {
    Pop-Location
}
