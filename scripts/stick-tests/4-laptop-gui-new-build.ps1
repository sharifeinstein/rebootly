# STEP 4 - on the Windows 10 laptop, after step 3. Tests the new
# Rebootly.exe (DirectX 12 by default, writes a start-up trace).
# Results: results\4-laptop-gui-new-build.txt
. (Join-Path $PSScriptRoot "_common.ps1")
$out = Join-Path $Results "4-laptop-gui-new-build.txt"
Remove-Item Env:\WGPU_BACKEND -ErrorAction SilentlyContinue
& {
    Machine-Info
    Gui-Test (Join-Path $Stick "\Rebootly.exe")
    Crash-Events
} *>&1 | Tee-Object -FilePath $out
Write-Host "`r`nSaved: $out" -ForegroundColor Green
