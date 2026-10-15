# Downloads the FFmpeg build Klipp is built with (LGPL, shared libraries) into
# vendor/ffmpeg and checks it against a fixed SHA-256, so every build ships
# exactly the same FFmpeg. It's kept as a release of the Klipp repository
# (ffmpeg-n8.1.3); that release says where it comes from and links its source.
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$repo = 'alonsy/klipp'
$tag = 'ffmpeg-n8.1.3'
$name = 'ffmpeg-n8.1.3-18-g46d899d7a0-win64-lgpl-shared'
$sha256 = 'dac1714fe5b419797bf46cec48e35567aab6fa84dbe5b6a17e3c3f508c077ac4'

$root = Split-Path $PSScriptRoot -Parent
$target = Join-Path $root 'vendor\ffmpeg'
if (Test-Path $target) { throw "$target already exists; delete it first to download again." }

$dl = Join-Path $env:TEMP "klipp-ffmpeg-$([guid]::NewGuid())"
New-Item -ItemType Directory $dl | Out-Null
try {
    $zip = Join-Path $dl "$name.zip"
    # The GitHub CLI also works on build servers and with a private fork.
    $viaCli = $false
    if (Get-Command gh -ErrorAction SilentlyContinue) {
        gh release download $tag --repo $repo --pattern "$name.zip" --dir $dl 2>$null
        $viaCli = $LASTEXITCODE -eq 0
    }
    if (-not $viaCli) {
        Invoke-WebRequest "https://github.com/$repo/releases/download/$tag/$name.zip" -OutFile $zip
    }
    $actual = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
    if ($actual -ne $sha256) { throw "Checksum mismatch: expected $sha256, got $actual" }

    Expand-Archive $zip -DestinationPath $dl
    New-Item -ItemType Directory -Force (Split-Path $target) | Out-Null
    Move-Item (Join-Path $dl $name) $target
    & (Join-Path $target 'bin\ffmpeg.exe') -hide_banner -version | Select-Object -First 1
} finally {
    Remove-Item -Recurse -Force $dl -ErrorAction SilentlyContinue
}
