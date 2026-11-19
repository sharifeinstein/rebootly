# STEP 8 - on any PC or VM (the laptop, your main PC, the VMware VM).
# Checks rebootly-cli's passphrase prompt (dots, paste, Esc) by backing up a
# tiny made-up Windows install from the stick (tests\fixture\win11-basic),
# so nothing of this PC is backed up. You type or paste at each step; the
# script says what to do and checks the result itself where it can.
# Work folder: %TEMP%\rebootly-passphrase-test (deleted at the end).
# Results: results\8-passphrase-<computer>.txt
. (Join-Path $PSScriptRoot "_common.ps1")
$out = Join-Path $Results "8-passphrase-$env:COMPUTERNAME.txt"
$cli = Join-Path $Stick "\bin\win-x64\rebootly-cli.exe"
$fixture = Join-Path $PSScriptRoot "fixture\win11-basic"
# Run from the repo (scripts\stick-tests): use the repo's build and fixture.
$repo = Join-Path $PSScriptRoot "..\.."
if (-not (Test-Path $cli)) { $cli = Join-Path $repo "target\release\rebootly-cli.exe" }
if (-not (Test-Path $fixture)) { $fixture = Join-Path $repo "tests\fixtures\win11-basic" }
$work = Join-Path $env:TEMP "rebootly-passphrase-test"
$usb = Join-Path $work "usb"
$typed = "typed-test-2026"
$pasted = "Pasted Test 2026!"
$script:pass = 0
$script:fail = 0

function Log($s) { Write-Host $s; Add-Content -Path $out -Value $s -Encoding UTF8 }
function Say($s) { Write-Host ""; Write-Host $s -ForegroundColor Yellow }
function Ask($q) {
    $a = Read-Host "$q Type y or n, then Enter"
    Log "  You answered: $a"
    return ($a -match '^\s*y')
}
function Result($name, [bool]$ok, $detail) {
    if ($ok) { $script:pass++; $w = "PASS" } else { $script:fail++; $w = "FAIL" }
    Log "$w  $name  $detail"
}
function Jobs { @(Get-ChildItem $usb -Directory -ErrorAction SilentlyContinue | Where-Object Name -ne "rebootly-profiles").Count }
# The prompt is drawn on stderr, so only stdout (the backup folder) is captured.
function Backup {
    $job = & $cli backup --windows-root $fixture --target $usb --user Alice --sensitive
    return @{ Code = $LASTEXITCODE; Job = ("" + ($job | Select-Object -Last 1)).Trim() }
}
# Opens the bundle with a known passphrase (no prompt): proves what arrived.
function Opens($job, $pass) {
    $env:REBOOTLY_PASSPHRASE = $pass
    $files = & $cli sensitive list (Join-Path $job "sensitive.age") 2>$null
    $code = $LASTEXITCODE
    Remove-Item Env:REBOOTLY_PASSPHRASE -ErrorAction SilentlyContinue
    return ($code -eq 0 -and @($files).Count -gt 0)
}
function Clear-Clip { cmd /c "type nul | clip" }

if ($Host.Name -ne "ConsoleHost") {
    Write-Host "Run this in a PowerShell window, not in the ISE or VS Code." -ForegroundColor Red
    exit 1
}
foreach ($p in @($cli, $fixture)) {
    if (-not (Test-Path $p)) {
        Write-Host "Missing: $p - run step 7 on your main PC first." -ForegroundColor Red
        exit 1
    }
}
Remove-Item Env:REBOOTLY_PASSPHRASE -ErrorAction SilentlyContinue
Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $usb | Out-Null
Set-Content -Path $out -Value "Step 8: passphrase prompt" -Encoding UTF8

Log ""
Log "Computer: $env:COMPUTERNAME   User: $env:USERNAME   Time: $(Get-Date -Format s)"
Log "Windows: $((Get-CimInstance Win32_OperatingSystem).Caption)"
Log "rebootly-cli: $cli"
Log "Version: $(& $cli --version)   (expected: c705d44 or newer)"
Log "Test install: $fixture"
Write-Host ""
Write-Host "Each step prints some lines about the test install first; the prompt" -ForegroundColor Cyan
Write-Host "comes after them. Test passphrases only - not a real one." -ForegroundColor Cyan

# A - typed, twice
Say "A. Typing. At 'Passphrase:' type   $typed   then Enter. At 'Passphrase again:' type it again, then Enter."
$r = Backup
Result "A typed backup" ($r.Code -eq 0 -and (Opens $r.Job $typed)) "(exit $($r.Code))"
$jobA = $r.Job
Result "A dots shown" (Ask "Did each character show as a dot with the count going up, and the line change to 'Passphrase: entered'?") ""

# B - typed to open the bundle
Say "B. Opening the bundle. At 'Passphrase:' type   $typed   then Enter."
$files = & $cli sensitive list (Join-Path $jobA "sensitive.age")
Result "B open typed" ($LASTEXITCODE -eq 0 -and @($files).Count -gt 0) "(exit $LASTEXITCODE, $(@($files).Count) files listed)"

# C - wrong passphrase
Say "C. Wrong passphrase. At 'Passphrase:' type   wrong-pass-2026   then Enter. It should be refused."
$null = & $cli sensitive list (Join-Path $jobA "sensitive.age")
Result "C wrong refused" ($LASTEXITCODE -ne 0) "(exit $LASTEXITCODE)"

# D - pasted with both shortcuts
Set-Clipboard -Value $pasted
Say "D. Pasting. This script put a test passphrase on the clipboard. At 'Passphrase:' press Ctrl+V, then Enter. At 'Passphrase again:' press Shift+Insert, then Enter."
$r = Backup
Clear-Clip
Result "D pasted backup" ($r.Code -eq 0 -and (Opens $r.Job $pasted)) "(exit $($r.Code); the bundle opens only if the paste arrived exactly)"
Result "D count shown" (Ask "After each paste, did the line show 17 characters?") ""

# E - Esc
$before = Jobs
Say "E. Cancelling. At 'Passphrase:' press Esc."
$r = Backup
Result "E Esc cancels" ($r.Code -eq 1 -and (Jobs) -eq $before) "(exit $($r.Code), no backup folder made)"
Result "E message" (Ask "Did it say 'cancelled; no passphrase was entered'?") ""

# F - too short
$before = Jobs
Say "F. Too short. At 'Passphrase:' type   abcde   then Enter."
$r = Backup
Result "F short refused" ($r.Code -eq 1 -and (Jobs) -eq $before) "(exit $($r.Code), no backup folder made)"
Result "F message" (Ask "Did it say 'the prompt received 5'?") ""

# G - the two don't match
$before = Jobs
Say "G. Not the same. At 'Passphrase:' type   $typed   then Enter. At 'Passphrase again:' type   typed-test-2025   then Enter."
$r = Backup
Result "G mismatch refused" ($r.Code -eq 1 -and (Jobs) -eq $before) "(exit $($r.Code), no backup folder made)"
Result "G message" (Ask "Did it say the passphrases don't match?") ""

# H - Ctrl+C (last: if Windows stops this script instead, the log ends here)
$before = Jobs
Log "H started (if nothing follows, Ctrl+C stopped the script itself)"
Say "H. Ctrl+C. At 'Passphrase:' press Ctrl+C."
$r = Backup
Result "H Ctrl+C cancels" ($r.Code -eq 1 -and (Jobs) -eq $before) "(exit $($r.Code), no backup folder made)"

Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
Log ""
Log "Done: $script:pass passed, $script:fail failed."
Write-Host "`r`nSaved: $out" -ForegroundColor Green
