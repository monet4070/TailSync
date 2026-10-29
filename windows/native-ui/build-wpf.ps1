param(
    [ValidateSet('x64', 'ARM64')][string]$Architecture = 'x64',
    [ValidateSet('history', 'favorites', 'settings', 'connections')][string]$Page = 'history',
    [switch]$Run,
    [switch]$SmokeTest,
    [switch]$Publish
)
$ErrorActionPreference = 'Stop'
Push-Location $PSScriptRoot
try {
    dotnet run --project TailSync.Native.Checks -c Release
    if ($LASTEXITCODE -ne 0) { throw 'Shared presentation checks failed' }
    $runtime = if ($Architecture -eq 'ARM64') { 'win-arm64' } else { 'win-x64' }
    $project = 'TailSync.Wpf/TailSync.Wpf.csproj'
    dotnet build $project -c Release -r $runtime
    if ($LASTEXITCODE -ne 0) { throw 'WPF build failed' }
    $binary = Join-Path $PSScriptRoot "TailSync.Wpf/bin/Release/net10.0-windows/$runtime/TailSync.Wpf.exe"
    if ($SmokeTest) {
        $report = Join-Path $PSScriptRoot "TailSync.Wpf/bin/Release/net10.0-windows/$runtime/wpf-smoke-results.json"
        $process = Start-Process -FilePath $binary -ArgumentList '--smoke-test', "--report=`"$report`"" -WorkingDirectory $PSScriptRoot -PassThru -Wait
        if ($process.ExitCode -ne 0) { throw "WPF runtime smoke checks failed; see $report" }
        Get-Content $report
    }
    if ($Publish) {
        dotnet publish $project -c Release -r $runtime --self-contained true -o "publish/wpf/$runtime"
        if ($LASTEXITCODE -ne 0) { throw 'WPF publish failed' }
    }
    if ($Run) { Start-Process -FilePath $binary -ArgumentList "--window=$Page" -WorkingDirectory $PSScriptRoot }
} finally { Pop-Location }
