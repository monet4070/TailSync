[CmdletBinding()]
param([string]$Target = 'x86_64-pc-windows-msvc')
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$Repository = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$HelperTarget = Join-Path $Repository 'target-clipboard-helper'
& cargo build --locked --release --manifest-path (Join-Path $Repository 'tools/clipboard-helper/Cargo.toml') --target $Target --target-dir $HelperTarget
if ($LASTEXITCODE -ne 0) { throw 'Clipboard helper build failed.' }
$Output = Join-Path $Repository 'windows/src-tauri/binaries'
New-Item -ItemType Directory -Force -Path $Output | Out-Null
Copy-Item -LiteralPath (Join-Path $HelperTarget "$Target/release/tailsync-clipboard-helper.exe") -Destination (Join-Path $Output "tailsync-clipboard-helper-$Target.exe")
Write-Output "Composite helper prepared for $Target. Build Tauri with --features protocol-composite-clipboard --config src-tauri/tauri.composite.conf.json."
