param(
    [ValidateSet('All', 'Pdfium', 'MuPdf', 'WindowsPdf')][string]$Engine = 'All',
    [switch]$Publish
)
$ErrorActionPreference = 'Stop'
Push-Location $PSScriptRoot
try {
    dotnet run --project Checks/Checks.csproj -c Release
    if ($LASTEXITCODE -ne 0) { throw 'Core checks failed.' }
    $engines = if ($Engine -eq 'All') { @('Pdfium', 'MuPdf', 'WindowsPdf') } else { @($Engine) }
    foreach ($name in $engines) {
        $project = "$name/$name.csproj"
        dotnet restore $project --locked-mode -r win-x64
        if ($LASTEXITCODE -ne 0) { throw "$name locked restore failed." }
        if ($Publish) {
            dotnet publish $project -c Release -r win-x64 --self-contained true -p:PublishReadyToRun=false -p:PublishTrimmed=false -o "publish/$name"
        } else {
            dotnet build $project -c Release -r win-x64 --no-restore
        }
        if ($LASTEXITCODE -ne 0) { throw "$name build failed." }
        if ($Publish) { Get-ChildItem "publish/$name" -Filter '*.pdb' -Recurse | Remove-Item }
    }
    if ($Publish) {
        Copy-Item README.md, run-demo.ps1, compare.ps1, prerequisites.ps1, summarize.py -Destination publish -Force
        Copy-Item fixtures -Destination publish -Recurse -Force
        Copy-Item licenses -Destination publish -Recurse -Force
        $files = Get-ChildItem publish -Recurse -File | Where-Object { $_.Name -ne 'sha256.json' -and $_.FullName -notmatch '[\\/]results[\\/]' }
        $manifest = @($files | ForEach-Object { [PSCustomObject]@{ path = $_.FullName.Substring((Join-Path $PSScriptRoot 'publish').Length + 1); bytes = $_.Length; sha256 = (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant() } })
        ConvertTo-Json -InputObject $manifest -Depth 4 | Set-Content publish/sha256.json -Encoding UTF8
        Write-Host 'Portable demos: publish/. No .NET install required on the target Windows x64 machine.'
    }
} finally { Pop-Location }
