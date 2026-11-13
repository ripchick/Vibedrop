# Fetch the external programs the Music section needs and put them next to the application.
#
#   pwsh scripts/fetch-tools.ps1 -Os windows|linux|macos -OutDir tools
#
# The programs are separate, independent open-source projects with their own licenses; they are
# shipped next to VibeDrop (not linked into it) so the Music section works right after the
# download. See THIRD_PARTY_NOTICES.md.

param(
  [ValidateSet("windows", "linux", "macos")][string]$Os = "windows",
  [string]$OutDir = "tools"
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

function Get-Remote([string]$Url, [string]$Out) {
  Write-Host "  downloading $Url"
  Invoke-WebRequest -Uri $Url -OutFile $Out -UseBasicParsing -TimeoutSec 1200
}

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $OutDir "licenses") | Out-Null

$ytVersion = "2026.10.26"
$ytBase = "https://github.com/yt-dlp/yt-dlp/releases/download/$ytVersion"

switch ($Os) {
  "windows" {
    Write-Host "yt-dlp for Windows"
    Get-Remote "$ytBase/yt-dlp.exe" (Join-Path $OutDir "yt-dlp.exe")

    Write-Host "ffmpeg for Windows (build by gyan.dev)"
    $zip = Join-Path $env:TEMP "ffmpeg-essentials.zip"
    Get-Remote "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip" $zip
    $tmp = Join-Path $env:TEMP "ffmpeg-essentials"
    if (Test-Path $tmp) { Remove-Item $tmp -Recurse -Force }
    Expand-Archive -Path $zip -DestinationPath $tmp -Force
    $bin = Get-ChildItem $tmp -Recurse -Filter "ffmpeg.exe" | Select-Object -First 1
    Copy-Item $bin.FullName (Join-Path $OutDir "ffmpeg.exe") -Force
    Get-ChildItem $tmp -Recurse -Include "LICENSE*", "README*", "*.txt" -File |
      Where-Object { $_.Name -match "license|readme" } |
      ForEach-Object { Copy-Item $_.FullName (Join-Path $OutDir "licenses") -Force }
    Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item $zip -Force -ErrorAction SilentlyContinue
  }
  "linux" {
    Write-Host "yt-dlp for Linux"
    Get-Remote "$ytBase/yt-dlp_linux" (Join-Path $OutDir "yt-dlp")
    & chmod +x (Join-Path $OutDir "yt-dlp")

    Write-Host "ffmpeg for Linux (static build by John Van Sickle)"
    $tar = Join-Path $env:TEMP "ffmpeg-static.tar.xz"
    Get-Remote "https://johnvansickle.com/ffmpeg/releases/ffmpeg-release-amd64-static.tar.xz" $tar
    $tmp = Join-Path $env:TEMP "ffmpeg-static"
    if (Test-Path $tmp) { Remove-Item $tmp -Recurse -Force }
    New-Item -ItemType Directory -Force -Path $tmp | Out-Null
    & tar -xJf $tar -C $tmp
    $bin = Get-ChildItem $tmp -Recurse -Filter "ffmpeg" -File | Select-Object -First 1
    Copy-Item $bin.FullName (Join-Path $OutDir "ffmpeg") -Force
    & chmod +x (Join-Path $OutDir "ffmpeg")
    Get-ChildItem $tmp -Recurse -Include "LICENSE*", "README*" -File |
      ForEach-Object { Copy-Item $_.FullName (Join-Path $OutDir "licenses") -Force }
    Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item $tar -Force -ErrorAction SilentlyContinue
  }
  "macos" {
    Write-Host "yt-dlp for macOS"
    Get-Remote "$ytBase/yt-dlp_macos" (Join-Path $OutDir "yt-dlp")
    & chmod +x (Join-Path $OutDir "yt-dlp")

    Write-Host "ffmpeg for macOS (build by evermeet.cx)"
    $zip = Join-Path $env:TEMP "ffmpeg-macos.zip"
    Get-Remote "https://evermeet.cx/ffmpeg/getrelease/zip" $zip
    $tmp = Join-Path $env:TEMP "ffmpeg-macos"
    if (Test-Path $tmp) { Remove-Item $tmp -Recurse -Force }
    Expand-Archive -Path $zip -DestinationPath $tmp -Force
    Copy-Item (Join-Path $tmp "ffmpeg") (Join-Path $OutDir "ffmpeg") -Force
    & chmod +x (Join-Path $OutDir "ffmpeg")
    Get-ChildItem $tmp -Include "LICENSE*", "README*" -File |
      ForEach-Object { Copy-Item $_.FullName (Join-Path $OutDir "licenses") -Force }
    Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item $zip -Force -ErrorAction SilentlyContinue
  }
}

Write-Host "tools in ${OutDir}:"
Get-ChildItem $OutDir -File | ForEach-Object { Write-Host ("  " + $_.Name + " " + [math]::Round($_.Length / 1MB, 1) + " MB") }

# Where the shipped binaries come from, so that the corresponding source can be found.
$build = @"
yt-dlp $ytVersion
  https://github.com/yt-dlp/yt-dlp/releases/download/$ytVersion/
  source: https://github.com/yt-dlp/yt-dlp

ffmpeg
"@
switch ($Os) {
  "windows" { $build += "  gyan.dev build (ffmpeg-release-essentials, GPL-2.0-or-later)`n  https://www.gyan.dev/ffmpeg/builds/`n  source: https://ffmpeg.org/download.html" }
  "linux" { $build += "  static build by John Van Sickle (GPL-2.0-or-later)`n  https://johnvansickle.com/ffmpeg/`n  source: https://ffmpeg.org/download.html" }
  "macos" { $build += "  build by evermeet.cx (GPL-2.0-or-later)`n  https://evermeet.cx/ffmpeg/`n  source: https://ffmpeg.org/download.html" }
}
$build | Set-Content -Path (Join-Path $OutDir "licenses/BUILD.txt") -Encoding UTF8
Write-Host "wrote licenses/BUILD.txt"

# The license of yt-dlp (the Unlicense).
$unlicense = @"
This is free and unencumbered software released into the public domain.

Anyone is free to copy, modify, publish, use, compile, sell, or distribute this
software, either in source code form or as a compiled binary, for any purpose,
commercial or non-commercial, and by any means.

In jurisdictions that recognize copyright laws, the author or authors of this
software dedicate any and all copyright interest in the software to the public
domain. We make this dedication for the benefit of the public at large and to
the detriment of our heirs and successors. We intend this dedication to be an
overt act of relinquishment in perpetuity of all present and future rights to
this software under copyright law.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN
ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION
WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

For more information, please refer to <https://unlicense.org/>
"@
$unlicense | Set-Content -Path (Join-Path $OutDir "licenses/yt-dlp-UNLICENSE.txt") -Encoding UTF8
Write-Host "wrote licenses/yt-dlp-UNLICENSE.txt"
