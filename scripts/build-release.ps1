<#
  Builds both Windows releases into .\release :
    DocShift-Setup.exe     installer (NSIS, per-user, no admin rights needed)
    DocShift-Portable.exe  single-file portable build (run from anywhere, nothing to install)
    SHA256SUMS.txt         checksums to publish next to them

  Usage:  powershell -ExecutionPolicy Bypass -File scripts\build-release.ps1
  Needs:  Node 20+, Rust (MSVC toolchain) and the Visual Studio C++ Build Tools.
#>
$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root

if (-not (Test-Path "src-tauri\binaries\ffmpeg.exe") -or -not (Test-Path "src-tauri\binaries\pdfium.dll")) {
  Write-Host "Fetching native engines (ffmpeg, pdfium)..." -ForegroundColor Cyan
  & "$PSScriptRoot\fetch-engines.ps1"
}
if (-not (Test-Path "node_modules")) {
  Write-Host "Installing npm packages..." -ForegroundColor Cyan
  npm ci
}

Write-Host "Building (first build takes several minutes)..." -ForegroundColor Cyan
npm run tauri build
if ($LASTEXITCODE -ne 0) { throw "tauri build failed" }

$release = Join-Path $root "release"
New-Item -ItemType Directory -Force $release | Out-Null
Remove-Item "$release\*" -Force -ErrorAction SilentlyContinue

$portable = "src-tauri\target\release\docshift.exe"
$setup = Get-ChildItem "src-tauri\target\release\bundle\nsis\*-setup.exe" | Sort-Object LastWriteTime -Descending | Select-Object -First 1
if (-not (Test-Path $portable)) { throw "Portable exe not found at $portable" }
if (-not $setup) { throw "NSIS installer not found" }

Copy-Item $portable "$release\DocShift-Portable.exe"
Copy-Item $setup.FullName "$release\DocShift-Setup.exe"

Get-FileHash "$release\*.exe" -Algorithm SHA256 |
  ForEach-Object { "{0}  {1}" -f $_.Hash.ToLower(), (Split-Path $_.Path -Leaf) } |
  Set-Content "$release\SHA256SUMS.txt" -Encoding ascii

Write-Host ""
Get-ChildItem $release | ForEach-Object { "{0,-26} {1,8:N1} MB" -f $_.Name, ($_.Length / 1MB) }
Write-Host "`nAttach these files to a GitHub release (tag v$((Get-Content package.json | ConvertFrom-Json).version))." -ForegroundColor Green
