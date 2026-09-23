# Bootstrap azooKey-Windows + mizuyokan overlay (PowerShell)
#   ./scripts/bootstrap-fork.ps1 [-Dest <path>]

param(
  [string]$Dest
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
if (-not $Dest) { $Dest = Join-Path $Root "azookey-windows-mizuyokan" }

$Upstream = "https://github.com/fkunn1326/azooKey-Windows.git"
$Pin = "65835aa1afd9ae7fafd7c58a86ea017877ebc58f"

if (-not (Test-Path $Dest)) {
  git clone --recursive $Upstream $Dest
  if ($LASTEXITCODE -ne 0) { throw "git clone failed" }
}

Push-Location $Dest
try {
  git fetch origin $Pin
  git checkout --force $Pin
  git submodule update --init --recursive
  if ($LASTEXITCODE -ne 0) { throw "checkout failed" }

  $EngineDst = Join-Path $Dest "mizuyokan-engine"
  if (Test-Path $EngineDst) { Remove-Item -Recurse -Force $EngineDst }
  New-Item -ItemType Directory -Path $EngineDst | Out-Null
  Copy-Item -Force (Join-Path $Root "engine\Cargo.toml") $EngineDst
  Copy-Item -Recurse -Force (Join-Path $Root "engine\src") $EngineDst

  $Overlay = Join-Path $Root "overlay"
  Get-ChildItem -Path $Overlay -Recurse -File | ForEach-Object {
    $rel = $_.FullName.Substring($Overlay.Length + 1)
    $target = Join-Path $Dest $rel
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $target) | Out-Null
    Copy-Item -Force $_.FullName $target
  }

  Write-Host "Ready: $Dest"
  Write-Host "Check: cargo build -p azookey-windows"
  Write-Host "Full : cargo make build --release  (see azooKey-Windows README)"
}
finally {
  Pop-Location
}
