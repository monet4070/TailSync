param(
    [string]$WinRoot,
    [string]$MacRoot
)

$ErrorActionPreference = 'Stop'
$arguments = @((Join-Path $PSScriptRoot 'check_cross_platform_sync.mjs'))
if ($WinRoot) { $arguments += @('--win-root', $WinRoot) }
if ($MacRoot) { $arguments += @('--mac-root', $MacRoot) }
& node @arguments
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}
