<#
.SYNOPSIS
  Puts Rebootly on a USB stick (normally one prepared with Ventoy), or
  builds the stick's folder for the release zip.

.DESCRIPTION
  Copies, from this repository and dist\:
    .rebootly-root, start.cmd, start.sh, README.txt   (rescue\stick\)
    ventoy\ventoy.json                                 (kept if you have your own)
    bin\win-x64\Rebootly.exe, rebootly-cli.exe        (target\release\)
    bin\win-arm64\...                                  (if built)
    bin\linux-x64\rebootly-cli                         (dist\linux-x64\, from rescue\alpine)
    iso\rebootly-rescue-x86_64.iso (+ .sources.txt)    (dist\, from rescue\alpine)
    iso\rebootly-winpe-amd64.iso                       (dist\, from rescue\winpe)
    packs\, GUIDE.txt, LICENSE.txt, THIRD-PARTY-LICENSES.html
  Backups made onto the stick go into its Rebootly\ folder; they are
  never touched.

.EXAMPLE
  .\rescue\make-stick.ps1 -Drive E:

.EXAMPLE
  .\rescue\make-stick.ps1 -OutDir dist\stick

.PARAMETER Drive
  The stick's data partition (Ventoy's is labelled "Ventoy").

.PARAMETER OutDir
  Build the stick's contents in this folder instead.

.PARAMETER Force
  Use a drive that doesn't look like a Ventoy stick (Rebootly then runs
  from it on Windows, but the PC can't start from it).
#>
param(
    [string]$Drive = "",
    [string]$OutDir = "",
    [switch]$Force
)
$ErrorActionPreference = "Stop"
$repo = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $repo

function Step($msg) { Write-Host "==> $msg" -ForegroundColor Cyan }
function Copy-To($src, $dir, $name = "") {
    # Not New-Item -Force on a drive root (E:\): Windows PowerShell rejects it.
    if (-not (Test-Path $dir)) { New-Item -ItemType Directory $dir | Out-Null }
    if ($name) { Copy-Item $src (Join-Path $dir $name) -Force } else { Copy-Item $src $dir -Force }
}

if (($Drive -and $OutDir) -or (-not $Drive -and -not $OutDir)) {
    throw "Give either -Drive E: or -OutDir <folder>."
}

if ($Drive) {
    $letter = $Drive.TrimEnd(':', '\').ToUpper()
    if ($letter.Length -ne 1) { throw "-Drive takes a drive letter, e.g. E:" }
    if ("$($letter):" -eq $env:SystemDrive) { throw "$($letter): is this PC's system drive." }
    $vol = Get-Volume -DriveLetter $letter -ErrorAction SilentlyContinue
    if (-not $vol) { throw "There is no drive $($letter):." }
    $ventoy = ($vol.FileSystemLabel -eq "Ventoy") -or (Test-Path "$($letter):\ventoy")
    if (-not $ventoy -and -not $Force) {
        throw ("$($letter): (`"$($vol.FileSystemLabel)`", $($vol.FileSystem)) doesn't look like a Ventoy stick. " +
            "Install Ventoy on it first (see rescue\README.md), or add -Force to use it anyway.")
    }
    if ($vol.FileSystem -notin @("exFAT", "NTFS")) {
        Write-Warning "$($letter): is $($vol.FileSystem): backups need exFAT or NTFS (FAT32 can't hold files over 4 GiB, and the rescue system only writes to exFAT/NTFS)."
    }
    $dest = "$($letter):\"
} else {
    New-Item -ItemType Directory -Force $OutDir | Out-Null
    $dest = (Resolve-Path $OutDir).Path
}

# Rebootly itself (Windows).
$exe = "target\release\Rebootly.exe", "target\release\rebootly-cli.exe"
foreach ($e in $exe) {
    if (-not (Test-Path $e)) { throw "$e is missing: build first (cargo build --release -p rebootly-gui -p rebootly-cli)." }
}
Step "Rebootly for Windows x64"
foreach ($e in $exe) { Copy-To $e (Join-Path $dest "bin\win-x64") }
$arm = "target\aarch64-pc-windows-msvc\release"
if (Test-Path "$arm\rebootly-cli.exe") {
    Step "Rebootly for Windows ARM64"
    foreach ($e in "Rebootly.exe", "rebootly-cli.exe") {
        if (Test-Path "$arm\$e") { Copy-To "$arm\$e" (Join-Path $dest "bin\win-arm64") }
    }
} else {
    Write-Host "    (no ARM64 build: cargo build --release --target aarch64-pc-windows-msvc)"
}

# Linux build and the rescue ISOs, when they have been built.
$linux = "dist\linux-x64\rebootly-cli"
if (Test-Path $linux) {
    Step "Rebootly for Linux x64"
    Copy-To $linux (Join-Path $dest "bin\linux-x64")
} else {
    Write-Warning "No $linux (rescue\alpine\build.sh makes it): start.sh won't work on other Linux systems."
}
$isos = @(
    @{ File = "dist\rebootly-rescue-x86_64.iso"; How = "rescue\alpine\build.sh" },
    @{ File = "dist\rebootly-winpe-amd64.iso"; How = "rescue\winpe\build.ps1" }
)
foreach ($i in $isos) {
    if (Test-Path $i.File) {
        Step "ISO $(Split-Path $i.File -Leaf)"
        Copy-To $i.File (Join-Path $dest "iso")
        $src = $i.File -replace '\.iso$', '.sources.txt'
        if (Test-Path $src) { Copy-To $src (Join-Path $dest "iso") }
    } else {
        Write-Host "    (no $($i.File): build it with $($i.How))"
    }
}

Step "Launchers, packs and guides"
Copy-To "rescue\stick\.rebootly-root" $dest
Copy-To "rescue\stick\start.cmd" $dest
Copy-To "rescue\stick\start.sh" $dest
Copy-To "rescue\stick\README.txt" $dest
Copy-To "docs\README-RELEASE.txt" $dest "GUIDE.txt"
Copy-To "LICENSE" $dest "LICENSE.txt"
Copy-To "THIRD-PARTY-LICENSES.html" $dest
New-Item -ItemType Directory -Force (Join-Path $dest "packs") | Out-Null
Copy-Item "packs\*.yaml" (Join-Path $dest "packs") -Force

# Ventoy settings: only search iso\ (backups hold many files) and let the
# rescue system write to the stick. Someone's own ventoy.json is kept.
$vj = Join-Path $dest "ventoy\ventoy.json"
if (Test-Path $vj) {
    $mine = Get-Content $vj -Raw
    $ours = Get-Content "rescue\stick\ventoy\ventoy.json" -Raw
    if ($mine -ne $ours) {
        Copy-To "rescue\stick\ventoy\ventoy.json" (Join-Path $dest "ventoy") "ventoy.json.rebootly"
        Write-Warning ("Kept your ventoy\ventoy.json. Add the settings from ventoy\ventoy.json.rebootly to it " +
            "(VTOY_DEFAULT_SEARCH_ROOT /iso and VTOY_LINUX_REMOUNT 1), or the rescue system can't save to the stick.")
    }
} else {
    Copy-To "rescue\stick\ventoy\ventoy.json" (Join-Path $dest "ventoy")
}
New-Item -ItemType Directory -Force (Join-Path $dest "Rebootly") | Out-Null

Write-Host ""
Write-Host "Rebootly is on $dest" -ForegroundColor Green
if ($Drive) { Write-Host "Eject the stick before unplugging it." }
