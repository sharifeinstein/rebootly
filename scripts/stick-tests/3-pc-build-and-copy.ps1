# STEP 3 - on your main PC, only if step 1 did NOT open the window.
# Builds the latest Rebootly from D:\Projects\rebootly and copies both exes
# to the stick root. Results: results\3-pc-build-and-copy.txt
param([string]$Repo = "D:\Projects\rebootly")
. (Join-Path $PSScriptRoot "_common.ps1")
$out = Join-Path $Results "3-pc-build-and-copy.txt"
& {
    Section "Build"
    Push-Location $Repo
    Native "git log -1 --oneline"
    Native "cargo build --release"
    "cargo exit code: $LASTEXITCODE"
    if ($LASTEXITCODE -eq 0) {
        Copy-Item target\release\Rebootly.exe, target\release\rebootly-cli.exe (Join-Path $Stick "\") -Force
        "Copied Rebootly.exe and rebootly-cli.exe to $Stick\"
    } else {
        "Build failed: nothing copied."
    }
    Pop-Location
} *>&1 | Tee-Object -FilePath $out
Write-Host "`r`nSaved: $out" -ForegroundColor Green
