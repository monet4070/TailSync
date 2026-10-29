param([ValidateSet('x64', 'ARM64')][string]$Architecture = 'x64', [switch]$Run, [ValidateSet('history', 'favorites', 'settings', 'connections')][string]$Page = 'history')
$ErrorActionPreference = 'Stop'
Push-Location $PSScriptRoot
try {
    dotnet run --project TailSync.Native.Checks -c Release
    if ($LASTEXITCODE -ne 0) { throw 'Native presentation checks failed' }
    dotnet build TailSync.Native/TailSync.Native.csproj -c Release "-p:Platform=$Architecture"
    if ($LASTEXITCODE -ne 0) { throw 'WinUI build failed' }
    if ($Run) {
        $binary = Get-ChildItem TailSync.Native/bin -Filter TailSync.Native.exe -Recurse |
            Where-Object { $_.FullName -match "[\\/]$Architecture[\\/]Release[\\/]" } |
            Sort-Object LastWriteTime -Descending | Select-Object -First 1
        if (-not $binary) { throw 'The compiled prototype executable was not found' }
        Start-Process -FilePath $binary.FullName -ArgumentList "--window=$Page" -WorkingDirectory $binary.DirectoryName
    }
} finally { Pop-Location }
