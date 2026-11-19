# STEP 6 - on the Windows 10 laptop, with Amouni SIGNED OUT.
# Short live backup of Amouni's Edge data (Edge pack + Links) to check that
# "Access is denied" files are now read from the shadow copy, plus the
# Windows licence check. Results: results\6-laptop-edge-vss.txt
param([string]$User = "Amouni")
. (Join-Path $PSScriptRoot "_common.ps1")
$out = Join-Path $Results "6-laptop-edge-vss.txt"
$cli = Join-Path $Stick "\bin\win-x64\rebootly-cli.exe"
$target = Join-Path $Stick "\Rebootly"
& {
    Machine-Info
    Section "Who is signed in ($User should NOT be listed)"
    Native "query user"
    Section "Edge processes per user"
    Get-Process msedge -IncludeUserName -ErrorAction SilentlyContinue | Group-Object UserName | Format-Table Name, Count -AutoSize | Out-String

    Section "rebootly-cli used"
    Get-Item $cli | Format-Table FullName, Length, LastWriteTime -AutoSize | Out-String

    Section "Live backup: $User, Edge pack + Links -> $target"
    & $cli backup --live --target $target --user $User --pack edge --folder Links 2>&1 |
        ForEach-Object { "$_" } | Select-Object -Last 15
    "backup exit code: $LASTEXITCODE"

    $job = Get-ChildItem $target -Directory | Sort-Object LastWriteTime | Select-Object -Last 1
    Section "Report: $($job.Name)"
    Select-String -Path (Join-Path $job.FullName "report.txt") -Pattern "Result:|failed|Access|edge|Why:" |
        ForEach-Object Line
    Section "Files read from the shadow copy (manifest)"
    $m = Get-Content (Join-Path $job.FullName "manifest.json") -Raw
    "snapshot: true entries: $(([regex]::Matches($m, '"snapshot"\s*:\s*true')).Count)"

    Section "Windows licence (last group and channel must match slmgr)"
    & $cli inspect --live 2>&1 | ForEach-Object { "$_" } | Select-String "licence" | ForEach-Object Line
    Native "cscript //nologo C:\Windows\System32\slmgr.vbs /dli" | Select-String "Description|Partial" | ForEach-Object Line
} *>&1 | Tee-Object -FilePath $out
Write-Host "`r`nSaved: $out" -ForegroundColor Green
