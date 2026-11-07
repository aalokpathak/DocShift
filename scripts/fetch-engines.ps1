<#
  Downloads the two native engines DocShift embeds, into src-tauri/binaries:
    - ffmpeg.exe  (video, HEIC/AVIF photos)  https://www.gyan.dev/ffmpeg/builds/  (GPL "essentials" build)
    - pdfium.dll  (PDF rendering/text)       https://github.com/bblanchon/pdfium-binaries (BSD/Apache)
  Run once after cloning:   powershell -ExecutionPolicy Bypass -File scripts/fetch-engines.ps1
#>
$ErrorActionPreference = "Stop"
# Windows PowerShell's progress bar slows big downloads to a crawl (and can make them time out)
$ProgressPreference = "SilentlyContinue"
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

$root = Split-Path $PSScriptRoot -Parent
$bin = Join-Path $root "src-tauri\binaries"
$tmp = Join-Path ([IO.Path]::GetTempPath()) ("docshift-engines-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Force $bin, $tmp | Out-Null

function Get-File([string]$url, [string]$out) {
  # curl.exe ships with Windows 10/11: fast, follows redirects, retries
  $curl = Join-Path $env:SystemRoot "System32\curl.exe"
  if (Test-Path $curl) {
    & $curl -L --fail --retry 3 --retry-delay 2 --connect-timeout 30 -sS -o $out $url
    if ($LASTEXITCODE -eq 0 -and (Test-Path $out)) { return }
    Write-Host "  curl failed (exit $LASTEXITCODE), trying Invoke-WebRequest..."
  }
  for ($i = 1; $i -le 3; $i++) {
    try {
      Invoke-WebRequest $url -OutFile $out -UseBasicParsing -TimeoutSec 600
      return
    } catch {
      Write-Host "  attempt $i failed: $($_.Exception.Message)"
      if ($i -eq 3) { throw "Couldn't download $url" }
      Start-Sleep -Seconds 3
    }
  }
}

try {
  Write-Host "Downloading pdfium..."
  Get-File "https://github.com/bblanchon/pdfium-binaries/releases/latest/download/pdfium-win-x64.tgz" "$tmp\pdfium.tgz"
  New-Item -ItemType Directory "$tmp\pdfium" | Out-Null
  # Windows' own bsdtar: a Git-for-Windows "tar" earlier in PATH misreads C:\ paths as remote hosts
  & "$env:SystemRoot\System32\tar.exe" -xzf "$tmp\pdfium.tgz" -C "$tmp\pdfium"
  if ($LASTEXITCODE -ne 0) { throw "Couldn't unpack pdfium" }
  Copy-Item "$tmp\pdfium\bin\pdfium.dll" "$bin\pdfium.dll" -Force
  Copy-Item "$tmp\pdfium\LICENSE" "$bin\PDFIUM-LICENSE.txt" -Force

  Write-Host "Downloading ffmpeg (about 115 MB)..."
  Get-File "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip" "$tmp\ffmpeg.zip"
  # tar.exe unpacks zip files too, and much faster than Expand-Archive
  New-Item -ItemType Directory "$tmp\ffmpeg" | Out-Null
  & "$env:SystemRoot\System32\tar.exe" -xf "$tmp\ffmpeg.zip" -C "$tmp\ffmpeg"
  if ($LASTEXITCODE -ne 0) { Expand-Archive "$tmp\ffmpeg.zip" -DestinationPath "$tmp\ffmpeg" -Force }
  $exe = Get-ChildItem "$tmp\ffmpeg" -Recurse -Filter ffmpeg.exe | Select-Object -First 1
  if (-not $exe) { throw "ffmpeg.exe wasn't found in the download" }
  Copy-Item $exe.FullName "$bin\ffmpeg.exe" -Force
  Write-Host "Done:" (Get-ChildItem $bin | ForEach-Object { "{0} ({1:N1} MB)" -f $_.Name, ($_.Length / 1MB) })
} finally {
  Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
}
