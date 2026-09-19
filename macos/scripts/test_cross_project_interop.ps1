param(
    [string]$WinRoot,
    [string]$MacRoot
)

$ErrorActionPreference = 'Stop'
$repositoryRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '../..')).Path
$canonicalScript = Join-Path $repositoryRoot 'windows/scripts/test_cross_project_interop.ps1'
$arguments = @()
if ($WinRoot) { $arguments += @('-WinRoot', $WinRoot) }
if ($MacRoot) { $arguments += @('-MacRoot', $MacRoot) }
& $canonicalScript @arguments
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}
