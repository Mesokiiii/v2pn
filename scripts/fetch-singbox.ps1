# Downloads and verifies sing-box and wintun, places them under
# crates/tauri-app/binaries/ ready for Tauri's externalBin/resources.
#
# Pinned versions and SHA-256 hashes are baked in. Do not bump blindly —
# regenerate after auditing the upstream release.

$ErrorActionPreference = "Stop"

$SingBoxVersion = "1.13.13"
$SingBoxZipUrl  = "https://github.com/SagerNet/sing-box/releases/download/v$SingBoxVersion/sing-box-$SingBoxVersion-windows-amd64.zip"
$SingBoxZipSha  = "AEA1FA983134A2E2D0600581D1178E98BD6BB93AE12AD8C333EAACAE68A1694C"

$WintunVersion = "0.14.1"
$WintunZipUrl  = "https://www.wintun.net/builds/wintun-$WintunVersion.zip"
$WintunZipSha  = "07C256185D6EE3652E09FA55C0B673E2624B565E02C4B9091C79CA7D2F24EF51"

$BinariesDir = Join-Path $PSScriptRoot "..\crates\tauri-app\binaries"
$Cache       = Join-Path $env:TEMP "v2pn-fetch-cache"

New-Item -ItemType Directory -Force -Path $BinariesDir | Out-Null
New-Item -ItemType Directory -Force -Path $Cache | Out-Null

function Download-Verify($url, $expectedSha, $outFile) {
  if (Test-Path $outFile) {
    $sha = (Get-FileHash $outFile -Algorithm SHA256).Hash
    if ($sha -ieq $expectedSha) {
      Write-Host "  cached and verified: $(Split-Path $outFile -Leaf)"
      return
    }
    Remove-Item $outFile -Force
  }
  Write-Host "  downloading $url"
  curl.exe -fsSL $url -o $outFile
  $sha = (Get-FileHash $outFile -Algorithm SHA256).Hash
  if ($sha -ine $expectedSha) {
    throw "SHA-256 mismatch for $url`n  expected: $expectedSha`n  got:      $sha"
  }
  Write-Host "  verified $(Split-Path $outFile -Leaf): $sha"
}

# ---- sing-box ---------------------------------------------------------------
Write-Host "==> sing-box $SingBoxVersion"
$sbZip = Join-Path $Cache "sing-box-$SingBoxVersion-windows-amd64.zip"
Download-Verify $SingBoxZipUrl $SingBoxZipSha $sbZip

$sbExtract = Join-Path $Cache "sing-box-$SingBoxVersion"
if (Test-Path $sbExtract) { Remove-Item -Recurse -Force $sbExtract }
Expand-Archive -Path $sbZip -DestinationPath $sbExtract -Force
$sbExe = Get-ChildItem -Path $sbExtract -Recurse -Filter "sing-box.exe" | Select-Object -First 1
if (-not $sbExe) { throw "sing-box.exe not found inside the archive" }

# Tauri's externalBin requires the binary file to carry the target triple.
# We name it sing-box-x86_64-pc-windows-msvc.exe so `externalBin: ["binaries/sing-box"]`
# resolves correctly on this host.
$dest = Join-Path $BinariesDir "sing-box-x86_64-pc-windows-msvc.exe"
Copy-Item $sbExe.FullName $dest -Force
Write-Host "  -> $dest ($([math]::Round((Get-Item $dest).Length / 1MB, 2)) MB)"

# ---- wintun -----------------------------------------------------------------
Write-Host "==> wintun $WintunVersion"
$wtZip = Join-Path $Cache "wintun-$WintunVersion.zip"
Download-Verify $WintunZipUrl $WintunZipSha $wtZip

$wtExtract = Join-Path $Cache "wintun-$WintunVersion"
if (Test-Path $wtExtract) { Remove-Item -Recurse -Force $wtExtract }
Expand-Archive -Path $wtZip -DestinationPath $wtExtract -Force
# We only need the amd64 build of wintun.dll (matches sing-box-windows-amd64).
$wtDll = Get-ChildItem -Path $wtExtract -Recurse -Filter "wintun.dll" |
         Where-Object { $_.FullName -match "amd64" } | Select-Object -First 1
if (-not $wtDll) { throw "amd64/wintun.dll not found inside the archive" }
Copy-Item $wtDll.FullName (Join-Path $BinariesDir "wintun.dll") -Force
Write-Host "  -> $(Join-Path $BinariesDir 'wintun.dll')"

Write-Host ""
Write-Host "Done. Files in binaries/:"
Get-ChildItem $BinariesDir | Format-Table Name, Length -AutoSize
