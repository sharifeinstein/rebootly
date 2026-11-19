# STEP 1 - on the Windows 10 laptop. Tests the Rebootly.exe that is on the
# stick now, forced to DirectX 12. Results: results\1-laptop-gui-dx12.txt
. (Join-Path $PSScriptRoot "_common.ps1")
$out = Join-Path $Results "1-laptop-gui-dx12.txt"
$env:WGPU_BACKEND = "dx12"
& {
    Machine-Info
    Gui-Test (Join-Path $Stick "\Rebootly.exe")
    Crash-Events
} *>&1 | Tee-Object -FilePath $out
Write-Host "`r`nSaved: $out" -ForegroundColor Green
