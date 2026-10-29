function Test-PreviewPrerequisites {
    param([string]$Engine)
    if (![Environment]::Is64BitOperatingSystem) { throw 'These demos require Windows x64.' }
    if ([Environment]::OSVersion.Version.Build -lt 19041) { throw 'Windows 10 build 19041 or newer is required.' }
    if ($Engine -eq 'MuPdf' -or $Engine -eq 'All') {
        foreach ($dll in @('msvcp140.dll', 'vcruntime140.dll', 'vcruntime140_1.dll')) {
            if (!(Test-Path (Join-Path $env:WINDIR "System32/$dll"))) {
                throw 'MuPDF requires Microsoft Visual C++ 2015-2022/compatible x64 Redistributable. Install the Microsoft-signed package from https://aka.ms/vs/17/release/vc_redist.x64.exe and retry. The demo does not install it automatically.'
            }
        }
    }
}
