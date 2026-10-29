param(
    [ValidateSet('Pdfium', 'MuPdf', 'WindowsPdf')][string]$Engine = 'Pdfium',
    [string]$File = '',
    [string]$Output = ''
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'prerequisites.ps1')
Test-PreviewPrerequisites -Engine $Engine
$root = if (Test-Path (Join-Path $PSScriptRoot "$Engine/TailSync.Preview.$Engine.exe")) { $PSScriptRoot } else { Join-Path $PSScriptRoot 'publish' }
$exe = Join-Path $root "$Engine/TailSync.Preview.$Engine.exe"
if (!(Test-Path $exe)) { throw 'Build the portable demos first: .\build.ps1 -Publish' }
if (!$Output) { $Output = Join-Path $root ('results/manual-' + $Engine + '-' + (Get-Date -Format 'yyyyMMdd-HHmmss-fff')) }
$arguments = @("--output=`"$([IO.Path]::GetFullPath($Output))`"")
if ($File) { $arguments += "--file=`"$((Resolve-Path $File).Path)`"" }
Start-Process -FilePath $exe -ArgumentList $arguments -WorkingDirectory (Split-Path $exe)
