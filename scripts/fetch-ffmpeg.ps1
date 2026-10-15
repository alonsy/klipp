# Downloads the FFmpeg 8.1 LGPL shared build (BtbN) into vendor/ffmpeg and verifies its checksum.
# Tested with n8.1.3-18-g46d899d7a0-20261013. Any 8.1.x build is ABI-compatible.
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$root = Split-Path $PSScriptRoot -Parent
$target = Join-Path $root 'vendor\ffmpeg'
if (Test-Path $target) { throw "$target already exists; delete it first to re-download." }

$name = 'ffmpeg-n8.1-latest-win64-lgpl-shared-8.1.zip'
$base = 'https://github.com/BtbN/FFmpeg-Builds/releases/download/latest'
$dl = Join-Path $env:TEMP "klipp-ffmpeg-$([guid]::NewGuid())"
New-Item -ItemType Directory $dl | Out-Null
try {
    Invoke-WebRequest "$base/$name" -OutFile (Join-Path $dl $name)
    Invoke-WebRequest "$base/checksums.sha256" -OutFile (Join-Path $dl 'checksums.sha256')
    $expected = (Select-String -Path (Join-Path $dl 'checksums.sha256') -Pattern ([regex]::Escape($name))).Line.Split(' ')[0]
    $actual = (Get-FileHash (Join-Path $dl $name) -Algorithm SHA256).Hash.ToLower()
    if ($expected -ne $actual) { throw "Checksum mismatch: expected $expected, got $actual" }

    Expand-Archive (Join-Path $dl $name) -DestinationPath $dl
    $extracted = Get-ChildItem $dl -Directory | Where-Object Name -like 'ffmpeg-n8.1*' | Select-Object -First 1
    New-Item -ItemType Directory -Force (Split-Path $target) | Out-Null
    Move-Item $extracted.FullName $target
    & (Join-Path $target 'bin\ffmpeg.exe') -hide_banner -version | Select-Object -First 1
} finally {
    Remove-Item -Recurse -Force $dl -ErrorAction SilentlyContinue
}
