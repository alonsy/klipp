# Removes Klipp: the program, its Start Menu shortcut and the start-with-Windows
# entry. Clips, settings and logs are kept; delete %APPDATA%\Klipp and
# %LOCALAPPDATA%\Klipp yourself if you want them gone.
param([string]$Dir = (Join-Path $env:LOCALAPPDATA 'Programs\Klipp'))
$ErrorActionPreference = 'Stop'

Get-Process klipp-recorder, klipp -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$Dir\*" } | Stop-Process -Force
Remove-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -Name 'Klipp' -ErrorAction SilentlyContinue
$menu = [Environment]::GetFolderPath('Programs')
Remove-Item (Join-Path $menu 'Klipp.lnk') -Force -ErrorAction SilentlyContinue
Remove-Item -Recurse -Force (Join-Path $menu 'Klipp') -ErrorAction SilentlyContinue
if (Test-Path $Dir) { Remove-Item -Recurse -Force $Dir }
Write-Host 'Klipp removed. Your clips and settings are still there.'
