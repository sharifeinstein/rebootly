<#
.SYNOPSIS
  Builds the Windows release zip: dist\Rebootly-<version>-windows-x64.zip
  (+ .sha256), with Rebootly.exe, rebootly-cli.exe, packs\, README.txt,
  LICENSE.txt and THIRD-PARTY-LICENSES.html; and the stick's contents,
  dist\Rebootly-<version>-stick.zip (unzip onto a Ventoy stick), with the
  Linux build and rescue ISOs from dist\ when they were built
  (rescue\alpine\build.sh, rescue\winpe\build.ps1).

.EXAMPLE
  .\scripts\package-release.ps1
  .\scripts\package-release.ps1 -DonateUrl ""   (no donation card)

.PARAMETER DonateUrl
  Link for the optional "buy me a coffee" card (default: the owner's
  Buy Me a Coffee page). An empty value leaves the card out.

.PARAMETER SkipTests
  Don't run `cargo test` first.
#>
param(
    [string]$DonateUrl = "https://www.buymeacoffee.com/sharifstuff",
    [switch]$SkipTests
)
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")

function Step($msg) { Write-Host "==> $msg" -ForegroundColor Cyan }
# Windows PowerShell 5.1 turns a native tool's stderr (progress lines) into an
# error record when output is redirected, and "Stop" then aborts the script.
# Native tools report failure by exit code, so run them under "Continue".
function Invoke-Native([string]$What, [scriptblock]$Run) {
    $ErrorActionPreference = "Continue"
    & $Run
    if ($LASTEXITCODE -ne 0) { throw "$What failed (exit code $LASTEXITCODE)" }
}

$m = Select-String -Path Cargo.toml -Pattern '^version = "(.+)"' | Select-Object -First 1
if (-not $m) { throw "version not found in Cargo.toml" }
$version = $m.Matches[0].Groups[1].Value

if (git status --porcelain) {
    Write-Warning "The working tree has uncommitted changes; the version line will name the last commit."
}

if (-not $SkipTests) {
    Step "cargo test"
    Invoke-Native "cargo test" { cargo test }
}

if ($DonateUrl) {
    $env:REBOOTLY_DONATE_URL = $DonateUrl
} else {
    Remove-Item Env:REBOOTLY_DONATE_URL -ErrorAction SilentlyContinue
    Write-Warning "No -DonateUrl: the donation card won't be shown."
}
# The link is compiled in; cargo rebuilds what reads it when it changes.
Step "cargo build --release"
Invoke-Native "cargo build" { cargo build --release -p rebootly-gui -p rebootly-cli }
Remove-Item Env:REBOOTLY_DONATE_URL -ErrorAction SilentlyContinue

$name = "Rebootly-$version-windows-x64"
$dist = Join-Path "dist" $name
Step "Packaging $name"
if (Test-Path $dist) { Remove-Item -Recurse -Force $dist }
New-Item -ItemType Directory -Force (Join-Path $dist "packs") | Out-Null
Copy-Item target\release\Rebootly.exe, target\release\rebootly-cli.exe $dist
Copy-Item packs\*.yaml (Join-Path $dist "packs")
Copy-Item docs\README-RELEASE.txt (Join-Path $dist "README.txt")
Copy-Item LICENSE (Join-Path $dist "LICENSE.txt")
Copy-Item THIRD-PARTY-LICENSES.html $dist

$zip = Join-Path "dist" "$name.zip"
if (Test-Path $zip) { Remove-Item $zip }
Compress-Archive -Path $dist -DestinationPath $zip
$hash = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
"$hash  $name.zip" | Set-Content (Join-Path "dist" "$name.zip.sha256") -Encoding ascii

$stickName = "Rebootly-$version-stick"
$stick = Join-Path "dist" $stickName
Step "Packaging $stickName"
if (Test-Path $stick) { Remove-Item -Recurse -Force $stick }
& (Join-Path $PSScriptRoot "..\rescue\make-stick.ps1") -OutDir $stick
$stickZip = Join-Path "dist" "$stickName.zip"
if (Test-Path $stickZip) { Remove-Item $stickZip }
# The folder's contents go to the zip's top level (they go onto the stick's root).
Compress-Archive -Path (Join-Path $stick "*") -DestinationPath $stickZip
$stickHash = (Get-FileHash $stickZip -Algorithm SHA256).Hash.ToLower()
"$stickHash  $stickName.zip" | Set-Content (Join-Path "dist" "$stickName.zip.sha256") -Encoding ascii

Invoke-Native "rebootly-cli --version" { & (Join-Path $dist "rebootly-cli.exe") --version }
Write-Host ""
Write-Host "Built $zip" -ForegroundColor Green
Write-Host "SHA-256 $hash"
Write-Host "Built $stickZip" -ForegroundColor Green
Write-Host "SHA-256 $stickHash"
