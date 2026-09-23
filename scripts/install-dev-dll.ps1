# Swap the IME DLLs of an installed azooKey-Windows with the mizuyokan build.
#   ./scripts/install-dev-dll.ps1            # build (release) and install
#   ./scripts/install-dev-dll.ps1 -Restore   # put back the DLLs from the official installer
#
# Requires azooKey-Windows v0.1.0-alpha1 installed via azookey-setup.exe.
# Between that release and the pinned upstream commit only the client DLL changed,
# so the installed server / launcher / UI stay compatible.

param(
  [switch]$Restore,
  [switch]$NoBuild
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
$Fork = Join-Path $Root "azookey-windows-mizuyokan"
$App = Join-Path $env:APPDATA "Azookey"

$Targets = @(
  @{ Name = "azookey.dll";   Built = Join-Path $Fork "target\release\azookey_windows.dll" },
  @{ Name = "azookey32.dll"; Built = Join-Path $Fork "target\i686-pc-windows-msvc\release\azookey_windows.dll" }
)

if (-not (Test-Path (Join-Path $App "azookey.dll"))) {
  throw "azooKey-Windows is not installed at $App. Run azookey-setup.exe first."
}

if (-not $Restore -and -not $NoBuild) {
  if (-not (Test-Path $Fork)) { throw "Fork not found. Run ./scripts/bootstrap-fork.ps1 first." }
  Push-Location $Fork
  try {
    cargo build -p azookey-windows --release
    if ($LASTEXITCODE -ne 0) { throw "x64 build failed" }
    cargo build -p azookey-windows --release --target i686-pc-windows-msvc
    if ($LASTEXITCODE -ne 0) { throw "x86 build failed" }
  }
  finally {
    Pop-Location
  }
}

$Stamp = Get-Date -Format "yyyyMMddHHmmss"
foreach ($t in $Targets) {
  $dest = Join-Path $App $t.Name
  $original = "$dest.original"

  # Keep the installer's DLL once so -Restore always has something to go back to.
  if (-not (Test-Path $original)) { Copy-Item $dest $original }

  $src = if ($Restore) { $original } else { $t.Built }
  if (-not (Test-Path $src)) { throw "Missing $src" }

  # A loaded DLL cannot be overwritten, but it can be renamed; running apps keep
  # the old image and newly started apps load the new file.
  $stale = "$dest.stale-$Stamp"
  Move-Item $dest $stale
  Copy-Item $src $dest
  icacls $dest /grant "*S-1-15-2-1:(RX)" | Out-Null
  Write-Host ("{0,-14} <- {1}" -f $t.Name, $src)
}

Get-ChildItem $App -Filter "*.stale-*" | ForEach-Object {
  try { Remove-Item $_.FullName -ErrorAction Stop } catch { }
}

Write-Host ""
Write-Host "Done. Restart the apps you want to test (e.g. close all Notepad windows)."
Write-Host "Mixed Japanese/English readings appear on Space once a key is set (scripts/set-jev-key.ps1)."
