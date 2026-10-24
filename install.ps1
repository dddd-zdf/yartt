# Builds yartt and installs it to %LOCALAPPDATA%\Programs\yartt, replacing a running copy.
# Run it again after pulling changes to update. Autostart (Settings) then points at the
# installed exe rather than the build folder, which `cargo build` replaces and `cargo clean` deletes.
#
#   powershell -ExecutionPolicy Bypass -File install.ps1

$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot

cargo build --release
if ($LASTEXITCODE -ne 0) { throw "Build failed" }

$dest = Join-Path $env:LOCALAPPDATA 'Programs\yartt'
New-Item -ItemType Directory -Force $dest | Out-Null

# The exe can't be replaced while it runs
Get-Process yartt -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 500

Copy-Item target\release\yartt.exe (Join-Path $dest 'yartt.exe') -Force
Start-Process (Join-Path $dest 'yartt.exe')
Write-Host "Installed and started $dest\yartt.exe"
