# Downloads the speech model "Klipp that" listens with into vendor/voice-model
# and checks it against a fixed SHA-256, so every build ships exactly the same
# model. It comes from sherpa-onnx's own releases; only the files Klipp uses
# are kept, under shorter names.
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$name = 'sherpa-onnx-kws-zipformer-zh-en-3M-2025-12-20'
$url = "https://github.com/k2-fsa/sherpa-onnx/releases/download/kws-models/$name.tar.bz2"
$sha256 = '68447f4fbc67e70eee3a93961f36e81e98f47aef73ce7e7ca00885c6cd3616a6'
$files = @{
    'encoder-epoch-13-avg-2-chunk-16-left-64.int8.onnx' = 'encoder.int8.onnx'
    'decoder-epoch-13-avg-2-chunk-16-left-64.onnx'      = 'decoder.onnx'
    'joiner-epoch-13-avg-2-chunk-16-left-64.int8.onnx'  = 'joiner.int8.onnx'
    'tokens.txt'                                        = 'tokens.txt'
}

$root = Split-Path $PSScriptRoot -Parent
$target = Join-Path $root 'vendor\voice-model'
if (Test-Path $target) { throw "$target already exists; delete it first to download again." }

$dl = Join-Path $env:TEMP "klipp-voice-model-$([guid]::NewGuid())"
New-Item -ItemType Directory $dl | Out-Null
try {
    $archive = Join-Path $dl "$name.tar.bz2"
    Invoke-WebRequest $url -OutFile $archive
    $actual = (Get-FileHash $archive -Algorithm SHA256).Hash.ToLower()
    if ($actual -ne $sha256) { throw "the model isn't the expected one: $actual" }
    tar -xf $archive -C $dl
    if ($LASTEXITCODE -ne 0) { throw 'unpacking the model failed' }
    New-Item -ItemType Directory -Force $target | Out-Null
    foreach ($f in $files.GetEnumerator()) {
        Copy-Item (Join-Path $dl "$name\$($f.Key)") (Join-Path $target $f.Value)
    }
} finally {
    Remove-Item -Recurse -Force $dl
}