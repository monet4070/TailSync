param(
    [string[]]$Files = @(),
    [ValidateRange(1, 20)][int]$Rounds = 3,
    [ValidateRange(1, 1000)][int]$Cycles = 3,
    [ValidateRange(128, 2048)][int]$Width = 1200,
    [ValidateRange(100, 60000)][int]$DwellMs = 500,
    [ValidateRange(100, 60000)][int]$IdleMs = 5000,
    [ValidateCount(1, 100)][int[]]$Pages = @(1, 2, 3),
    [ValidateRange(1, 120)][int]$MaxMinutes = 30,
    [string]$Output = ''
)
$ErrorActionPreference = 'Stop'
if (@($Pages | Where-Object { $_ -lt 1 -or $_ -gt 100000 }).Count -gt 0) { throw 'Pages must be one-based numbers from 1 to 100000.' }
. (Join-Path $PSScriptRoot 'prerequisites.ps1')
Test-PreviewPrerequisites -Engine All
if (@(Get-CimInstance Win32_Processor | Where-Object Architecture -eq 12).Count -gt 0) {
    throw 'This comparison bundle is x64. Do not mix ARM64 emulation results with native x64 measurements.'
}
$root = if (Test-Path (Join-Path $PSScriptRoot 'Pdfium/TailSync.Preview.Pdfium.exe')) { $PSScriptRoot } else { Join-Path $PSScriptRoot 'publish' }
if (!$Output) { $Output = Join-Path $root ('results/compare-' + (Get-Date -Format 'yyyyMMdd-HHmmss-fff')) }
$Output = [IO.Path]::GetFullPath($Output)
if (Test-Path $Output) { throw 'Choose a new output folder; existing measurements will not be overwritten.' }
New-Item $Output -ItemType Directory | Out-Null
if ($Files.Count -eq 0) { $Files = @(Get-ChildItem (Join-Path $root 'fixtures') -File | Where-Object { $_.Extension -in @('.pdf', '.docx') } | Sort-Object Name | ForEach-Object FullName) }
if ($Files.Count -eq 0) { throw 'No input files found.' }
foreach ($name in @('Pdfium', 'MuPdf', 'WindowsPdf')) {
    if (!(Test-Path (Join-Path $root "$name/TailSync.Preview.$name.exe"))) { throw "Missing demo $name; run build.ps1 -Publish." }
}
$environment = [PSCustomObject]@{
    recorded_at = (Get-Date).ToUniversalTime().ToString('o')
    os = (Get-CimInstance Win32_OperatingSystem | Select-Object Caption, Version, BuildNumber, TotalVisibleMemorySize)
    cpu = @(Get-CimInstance Win32_Processor | Select-Object Name, NumberOfCores, NumberOfLogicalProcessors)
    gpu = @(Get-CimInstance Win32_VideoController | Select-Object Name, DriverVersion)
    rounds = $Rounds; cycles = $Cycles; pixel_width = $Width; dwell_ms = $DwellMs; idle_ms = $IdleMs; pages = $Pages
    note = 'Sequential runs with rotated engine order. No OS file-cache flush; first run is process-cold, not guaranteed disk-cold. GPU memory is not sampled. Keep windows visible.'
}
$environment | ConvertTo-Json -Depth 8 | Set-Content (Join-Path $Output 'environment.json') -Encoding UTF8
$runs = [Collections.Generic.List[object]]::new()
$failures = 0
foreach ($inputFile in $Files) {
    $file = (Resolve-Path $inputFile).Path
    $extension = [IO.Path]::GetExtension($file).ToLowerInvariant()
    $engines = if ($extension -eq '.pdf') { @('Pdfium', 'MuPdf', 'WindowsPdf') } elseif ($extension -in @('.docx', '.pptx', '.xlsx')) { @('MuPdf') } else { throw "Unsupported input: $file" }
    $hash = (Get-FileHash $file -Algorithm SHA256).Hash.ToLowerInvariant()
    for ($round = 0; $round -lt $Rounds; $round++) {
        for ($j = 0; $j -lt $engines.Count; $j++) {
            $engine = $engines[($j + $round) % $engines.Count]
            $name = ('{0:D3}-{1}-{2}' -f $runs.Count, $engine, $hash.Substring(0, 12))
            $runOutput = Join-Path $Output $name
            $exe = Join-Path $root "$engine/TailSync.Preview.$engine.exe"
            $arguments = @('--benchmark', "--file=`"$file`"", "--output=`"$runOutput`"", "--cycles=$Cycles", "--width=$Width", "--dwell-ms=$DwellMs", "--idle-ms=$IdleMs", "--pages=$($Pages -join ',')")
            Write-Host "$engine / $([IO.Path]::GetFileName($file)) / round $($round + 1)"
            $process = Start-Process -FilePath $exe -ArgumentList $arguments -WorkingDirectory (Split-Path $exe) -PassThru
            $timedOut = !$process.WaitForExit($MaxMinutes * 60000)
            if ($timedOut) { Stop-Process -Id $process.Id -Force; $process.WaitForExit(); $code = -1 } else { $code = $process.ExitCode }
            if ($code -ne 0) { $failures++; Write-Warning "Run failed ($code): $runOutput" }
            $runs.Add([PSCustomObject]@{ engine = $engine; file = [IO.Path]::GetFileName($file); sha256 = $hash; round = $round; output = $name; exit_code = $code; timed_out = $timedOut })
            ConvertTo-Json -InputObject @($runs.ToArray()) -Depth 5 | Set-Content (Join-Path $Output 'runs.json') -Encoding UTF8
            $process.Dispose()
        }
    }
}
Write-Host "Results: $Output"
Write-Host "Optional summary: python summarize.py `"$Output`""
if ($failures -gt 0) { throw "$failures runs failed. Preserve their logs; do not compare failed runs as successful ones." }
