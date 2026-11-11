<#
.SYNOPSIS
  Builds the Rebootly WinPE rescue ISO: dist\rebootly-winpe-<arch>.iso.

.DESCRIPTION
  WinPE can't be redistributed, so everyone builds their own (SPEC section 8).
  Needs, on this PC: the Windows ADK and its "Windows PE add-on"
  (https://learn.microsoft.com/windows-hardware/get-started/adk-install),
  and an elevated PowerShell (DISM).

  The image is generic: Rebootly itself isn't in it. At start it looks for
  the Rebootly stick (a drive with .rebootly-root) and runs its start.cmd,
  so updating Rebootly on the stick needs no new ISO.

  Added to WinPE: WMI, .NET, scripting and PowerShell (BitLocker password
  unlock goes through WMI/PowerShell), storage WMI, Enhanced Storage and
  Secure Startup (manage-bde, BitLocker), 512 MB scratch space.

.EXAMPLE
  .\rescue\winpe\build.ps1

.EXAMPLE
  .\rescue\winpe\build.ps1 -Arch arm64

.PARAMETER Arch
  amd64 (default) or arm64 (WinPE has no x64 emulation on ARM).

.PARAMETER WorkDir
  Scratch folder (default C:\WinPE_rebootly_<arch>); deleted first.

.PARAMETER Out
  The ISO (default dist\rebootly-winpe-<arch>.iso).

.PARAMETER Lang
  WinPE language packs (default en-us).
#>
param(
    [ValidateSet("amd64", "arm64")][string]$Arch = "amd64",
    [string]$WorkDir = "",
    [string]$Out = "",
    [string]$Lang = "en-us"
)
$ErrorActionPreference = "Stop"
$repo = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
if (-not $WorkDir) { $WorkDir = "C:\WinPE_rebootly_$Arch" }
if (-not $Out) { $Out = Join-Path $repo "dist\rebootly-winpe-$Arch.iso" }
$Out = [IO.Path]::GetFullPath($Out)

function Step($msg) { Write-Host "==> $msg" -ForegroundColor Cyan }

$admin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole(
    [Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $admin) { throw "Run this from an elevated PowerShell (Run as administrator): DISM needs it." }

# The ADK.
$roots = Get-ItemProperty "HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows Kits\Installed Roots" -ErrorAction SilentlyContinue
$kits = if ($roots -and $roots.KitsRoot10) { $roots.KitsRoot10 } else { "C:\Program Files (x86)\Windows Kits\10\" }
$adk = Join-Path $kits "Assessment and Deployment Kit"
$dandi = Join-Path $adk "Deployment Tools\DandISetEnv.bat"
$pe = Join-Path $adk "Windows Preinstallation Environment"
$ocs = Join-Path $pe "$Arch\WinPE_OCs"
if (-not (Test-Path $dandi)) {
    throw "The Windows ADK isn't installed (no $dandi). Install the ADK (Deployment Tools) first."
}
if (-not (Test-Path $ocs)) {
    throw "The Windows PE add-on for the ADK isn't installed (no $ocs). Install it (same page as the ADK)."
}
$hostArch = if ($env:PROCESSOR_ARCHITECTURE -eq "ARM64") { "arm64" } else { "amd64" }
$dism = Join-Path $adk "Deployment Tools\$hostArch\DISM\dism.exe"
if (-not (Test-Path $dism)) { $dism = "dism.exe" }

function Invoke-Dism([string[]]$a) {
    & $dism /English @a
    if ($LASTEXITCODE -ne 0) { throw "dism $($a -join ' ') failed (exit code $LASTEXITCODE)" }
}
# copype and MakeWinPEMedia need the ADK's environment (paths to oscdimg etc.).
function AdkCmd([string]$line) {
    cmd /c "call `"$dandi`" >nul && $line"
    if ($LASTEXITCODE -ne 0) { throw "$line failed (exit code $LASTEXITCODE)" }
}

$mount = Join-Path $WorkDir "mount"
if (Test-Path $WorkDir) {
    Step "Removing the old $WorkDir"
    if (Test-Path (Join-Path $mount "Windows")) {
        & $dism /English /Unmount-Image /MountDir:"$mount" /Discard | Out-Null
    }
    & $dism /English /Cleanup-Mountpoints | Out-Null
    Remove-Item -Recurse -Force $WorkDir
}

Step "copype $Arch $WorkDir"
AdkCmd "copype $Arch `"$WorkDir`""

$wim = Join-Path $WorkDir "media\sources\boot.wim"
Step "Mounting boot.wim"
Invoke-Dism @("/Mount-Image", "/ImageFile:$wim", "/Index:1", "/MountDir:$mount")
$committed = $false
try {
    # Order matters: each needs the ones before it.
    $packages = "WinPE-WMI", "WinPE-NetFX", "WinPE-Scripting", "WinPE-PowerShell",
        "WinPE-StorageWMI", "WinPE-EnhancedStorage", "WinPE-SecureStartup"
    foreach ($p in $packages) {
        $cab = Join-Path $ocs "$p.cab"
        if (-not (Test-Path $cab)) {
            Write-Warning "$p isn't in this ADK for ${Arch}: BitLocker password unlock may not work."
            continue
        }
        Step "Adding $p"
        Invoke-Dism @("/Image:$mount", "/Add-Package", "/PackagePath:$cab")
        $langCab = Join-Path $ocs "$Lang\$($p)_$Lang.cab"
        if (Test-Path $langCab) {
            Invoke-Dism @("/Image:$mount", "/Add-Package", "/PackagePath:$langCab")
        }
    }
    Step "Scratch space 512 MB (PowerShell needs more than the default)"
    Invoke-Dism @("/Image:$mount", "/Set-ScratchSpace:512")

    Step "Start scripts"
    $sys32 = Join-Path $mount "Windows\System32"
    $startnet = Get-Content (Join-Path $PSScriptRoot "startnet.cmd") -Raw
    $find = Get-Content (Join-Path $PSScriptRoot "rebootly.cmd") -Raw
    # cmd needs CRLF line ends.
    [IO.File]::WriteAllText((Join-Path $sys32 "startnet.cmd"), ($startnet -replace "`r?`n", "`r`n"))
    [IO.File]::WriteAllText((Join-Path $sys32 "rebootly.cmd"), ($find -replace "`r?`n", "`r`n"))

    Step "Saving the image"
    Invoke-Dism @("/Unmount-Image", "/MountDir:$mount", "/Commit")
    $committed = $true
} finally {
    if (-not $committed) {
        Write-Warning "Something failed: discarding the mounted image."
        & $dism /English /Unmount-Image /MountDir:"$mount" /Discard | Out-Null
    }
}

Step "Making $Out"
New-Item -ItemType Directory -Force (Split-Path $Out) | Out-Null
if (Test-Path $Out) { Remove-Item -Force $Out }
AdkCmd "MakeWinPEMedia /ISO `"$WorkDir`" `"$Out`""
$hash = (Get-FileHash $Out -Algorithm SHA256).Hash.ToLower()
"$hash  $(Split-Path $Out -Leaf)" | Set-Content "$Out.sha256" -Encoding ascii

Write-Host ""
Write-Host "Built $Out" -ForegroundColor Green
Write-Host "Put it on the stick: .\rescue\make-stick.ps1 -Drive E:   (it copies dist\*.iso to iso\)"
Write-Host "You can delete $WorkDir."
