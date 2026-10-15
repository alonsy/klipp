# Builds Klipp and installs it for the current user (no admin needed).
#
#   .\scripts\install.ps1               # build, install, Start Menu shortcut, start recording in the tray
#   .\scripts\install.ps1 -Autostart    # ...and start Klipp's recorder with Windows
#   .\scripts\install.ps1 -NoBuild      # install what's already in target\release
#
# Run it again to update. Clips and settings are kept.
param(
    [string]$Dir = (Join-Path $env:LOCALAPPDATA 'Programs\Klipp'),
    [switch]$NoBuild,
    [switch]$Autostart,
    [switch]$NoShortcuts,
    [switch]$NoStart
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$release = Join-Path $root 'target\release'
$runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'

if (-not $NoBuild) {
    Write-Host 'Building the interface...'
    Push-Location (Join-Path $root 'studio')
    try {
        npm install --no-fund --no-audit | Out-Null
        npm run build | Out-Null
        if ($LASTEXITCODE -ne 0) { throw 'frontend build failed' }
    } finally { Pop-Location }
    Write-Host 'Building Klipp...'
    $cargo = Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe'
    & $cargo build --release --manifest-path (Join-Path $root 'Cargo.toml')
    if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }
}

$hadAutostart = $false
$hadAutostart = $hadAutostart -or ($null -ne (Get-ItemProperty $runKey -ErrorAction SilentlyContinue).'Klipp')

# Files can't be replaced while Klipp runs, and only one recorder can run,
# so any running copy stops (wherever it was started from).
$running = Get-Process klipp-recorder, klipp -ErrorAction SilentlyContinue
if ($running) {
    Write-Host 'Stopping the running copy...'
    $running | ForEach-Object { $_.CloseMainWindow() | Out-Null }
    Start-Sleep -Seconds 2
    $running | Where-Object { -not $_.HasExited } | Stop-Process -Force
    # Windows releases the files a moment after the process is gone.
    $running | ForEach-Object { $_.WaitForExit(10000) | Out-Null }
}

Write-Host "Installing to $Dir"
New-Item -ItemType Directory -Force $Dir | Out-Null
$files = @('klipp.exe', 'klipp-recorder.exe', 'ffmpeg.exe', 'ffprobe.exe') + (Get-ChildItem $release -Filter '*.dll' | ForEach-Object Name)
foreach ($f in $files) {
    $src = Join-Path $release $f
    if (-not (Test-Path $src)) { throw "missing $src (build first, or drop -NoBuild)" }
    for ($try = 1; ; $try++) {
        try { Copy-Item $src $Dir -Force -ErrorAction Stop; break }
        catch { if ($try -ge 10) { throw }; Start-Sleep -Milliseconds 500 }
    }
}
# FFmpeg is LGPL: its license travels with it.
Copy-Item (Join-Path $root 'vendor\ffmpeg\LICENSE.txt') (Join-Path $Dir 'FFMPEG-LICENSE.txt') -Force

if (-not $NoShortcuts) {
    $menu = [Environment]::GetFolderPath('Programs')
    Remove-Item -Recurse -Force (Join-Path $menu 'Klipp') -ErrorAction SilentlyContinue   # older layout
    $lnk = (New-Object -ComObject WScript.Shell).CreateShortcut((Join-Path $menu 'Klipp.lnk'))
    $lnk.TargetPath = Join-Path $Dir 'klipp.exe'
    $lnk.WorkingDirectory = $Dir
    $lnk.Description = 'Klipp: game clips without the memory leaks'
    $lnk.Save()
    Write-Host 'Start Menu: Klipp'
}

if ($Autostart -or $hadAutostart) {
    # Only the recorder starts with Windows; the window opens when you launch Klipp.
    Set-ItemProperty $runKey -Name 'Klipp' -Value "`"$(Join-Path $Dir 'klipp-recorder.exe')`""
    Write-Host 'Klipp will start recording with Windows.'
}

if (-not $NoStart) {
    Start-Process (Join-Path $Dir 'klipp-recorder.exe')
    Write-Host 'Klipp is recording. Open it from the Start Menu, or double-click the tray icon.'
}
Write-Host 'Done.'
