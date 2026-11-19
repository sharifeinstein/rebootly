# STEP 2 - on the Windows 10 laptop. Why Amouni's Edge files were
# "Access is denied" in the live backup. Results: results\2-laptop-edge-access.txt
param([string]$User = "Amouni")
. (Join-Path $PSScriptRoot "_common.ps1")
$out = Join-Path $Results "2-laptop-edge-access.txt"
$d = "C:\Users\$User\AppData\Local\Microsoft\Edge\User Data\Default"
$tmp = Join-Path $env:TEMP "rb-edge-test"
& {
    Machine-Info
    Section "Who is signed in"
    Native "query user"
    Section "Edge processes per user"
    Get-Process msedge -IncludeUserName -ErrorAction SilentlyContinue | Group-Object UserName | Format-Table Name, Count -AutoSize | Out-String
    Section "Backup privilege of this window"
    Native "whoami /priv" | Select-String "SeBackupPrivilege|SeRestorePrivilege"
    Section "Permissions"
    Native "icacls `"$d`""
    Native "icacls `"$d\Bookmarks`""
    Section "robocopy /B (backup mode) of Bookmarks"
    Native "robocopy `"$d`" `"$tmp`" Bookmarks /B /R:0 /W:0 /NJH /NP"
    Section "robocopy without /B"
    Native "robocopy `"$d`" `"$tmp`" Bookmarks /R:0 /W:0 /NJH /NP"
    Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
    Section "Antivirus"
    Get-CimInstance -Namespace root/SecurityCenter2 -ClassName AntivirusProduct -ErrorAction SilentlyContinue | Format-Table displayName, productState -AutoSize | Out-String
    Section "Defender blocks (controlled folder access etc.)"
    Get-WinEvent -LogName "Microsoft-Windows-Windows Defender/Operational" -MaxEvents 300 -ErrorAction SilentlyContinue |
        Where-Object { $_.Id -in 1123,1124,1127 } | Select-Object -First 5 | Format-List TimeCreated, Message | Out-String -Width 200
} *>&1 | Tee-Object -FilePath $out
Write-Host "`r`nSaved: $out" -ForegroundColor Green
