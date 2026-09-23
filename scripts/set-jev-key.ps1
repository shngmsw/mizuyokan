# Store the Jev (AI gateway) API key for mizuyokan, encrypted with DPAPI for the current user.
#   ./scripts/set-jev-key.ps1 -OpRef "op://Private/Lolipop AI Gateway/credential"   # from 1Password CLI
#   ./scripts/set-jev-key.ps1                                                      # prompt (hidden input)
#   ./scripts/set-jev-key.ps1 -Test                                                # call Jev once with the stored key
#   ./scripts/set-jev-key.ps1 -Remove                                              # back to plain azooKey
#
# The key is never printed or written in plain text. Settings live in
# %APPDATA%\Azookey\mizuyokan.json next to azooKey's own settings.json.

param(
  [string]$OpRef,
  [switch]$Test,
  [switch]$Remove
)

$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Security

$Dir = Join-Path $env:APPDATA "Azookey"
$Path = Join-Path $Dir "mizuyokan.json"

function Read-Settings {
  if (Test-Path $Path) {
    $parsed = Get-Content $Path -Raw | ConvertFrom-Json -AsHashtable
    if ($parsed) { return $parsed }
  }
  return @{}
}

function Write-Settings($settings) {
  New-Item -ItemType Directory -Force -Path $Dir | Out-Null
  $json = $settings | ConvertTo-Json -Depth 4
  [IO.File]::WriteAllText($Path, $json, (New-Object System.Text.UTF8Encoding($false)))
}

function Get-StoredKey($settings) {
  if (-not $settings.jev_api_key_dpapi) { throw "No key stored. Run without -Test first." }
  $bytes = [Security.Cryptography.ProtectedData]::Unprotect(
    [Convert]::FromBase64String($settings.jev_api_key_dpapi), $null, "CurrentUser")
  return [Text.Encoding]::UTF8.GetString($bytes)
}

$settings = Read-Settings

if ($Remove) {
  $settings.Remove("jev_api_key_dpapi")
  Write-Settings $settings
  Write-Host "Removed. mizuyokan now behaves like plain azooKey."
  return
}

if ($Test) {
  $key = Get-StoredKey $settings
  $endpoint = if ($settings.jev_endpoint) { $settings.jev_endpoint } else { "https://ai-gateway.lolipop.jp/v1/systemone" }
  $model = if ($settings.jev_model) { $settings.jev_model } else { "typesafe/jev-latest" }
  $body = @{
    model     = $model
    state     = @{ raw = "gitpullshitara" }
    questions = @{
      reading = @{
        type         = "choice"
        instructions = "A user typed the keystrokes in raw into a Japanese IME without switching between Japanese and English. Which option is the text the user most likely intended?"
        criteria     = @{ o0 = "git pullしたら"; o1 = "ぎtぷllしたら" }
      }
    }
  } | ConvertTo-Json -Depth 6
  # The first call includes the TLS handshake; later ones show the warm latency the IME sees.
  foreach ($i in 1..3) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $resp = Invoke-RestMethod -Method Post -Uri $endpoint -Body ([Text.Encoding]::UTF8.GetBytes($body)) `
      -ContentType "application/json; charset=utf-8" -Headers @{ Authorization = "Bearer $key" }
    $sw.Stop()
    $answer = $resp.answers.reading
    Write-Host ("#{0} model={1} choice={2} probabilities={3} ({4} ms)" -f `
        $i, $resp.model, $answer.choice, ($answer.probabilities | ConvertTo-Json -Compress), $sw.ElapsedMilliseconds)
  }
  return
}

if ($OpRef) {
  $key = op read $OpRef
  if ($LASTEXITCODE -ne 0 -or -not $key) { throw "op read failed for $OpRef" }
} else {
  $secure = Read-Host -AsSecureString "Jev / AI gateway API key"
  $key = [Net.NetworkCredential]::new("", $secure).Password
}
$key = $key.Trim()
if (-not $key) { throw "Empty key" }

$protected = [Security.Cryptography.ProtectedData]::Protect(
  [Text.Encoding]::UTF8.GetBytes($key), $null, "CurrentUser")
$settings.jev_api_key_dpapi = [Convert]::ToBase64String($protected)
Write-Settings $settings
Remove-Variable key
Write-Host "Saved to $Path (DPAPI, current user only)."
