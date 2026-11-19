# STEP 5 - on your main PC (stick plugged in). Tests, clippy and a release
# build of D:\Projects\rebootly, then copies both exes to the stick's
# bin\win-x64\. Results: results\5-pc-build-test-copy.txt
param([string]$Repo = "D:\Projects\rebootly")
. (Join-Path $PSScriptRoot "_common.ps1")
$out = Join-Path $Results "5-pc-build-test-copy.txt"
$bin = Join-Path $Stick "\bin\win-x64"
& {
    Section "Repo"
    Push-Location $Repo
    Native "git log -1 --oneline"
    "(expected: 345d33a or newer)"

    Section "cargo test"
    $t = Native "cargo test -q"
    $code = $LASTEXITCODE
    $t | Select-String "test result|FAILED|panicked|^error" |
        Where-Object { $_.Line -notmatch "ok. 0 passed" } | ForEach-Object Line
    "cargo test exit code: $code"

    Section "cargo clippy"
    $c = Native "cargo clippy -q --all-targets -- -D warnings"
    "clippy exit code: $LASTEXITCODE"
    $c | Select-Object -Last 20

    Section "cargo build --release"
    $b = Native "cargo build --release -q"
    $code = $LASTEXITCODE
    $b | Select-Object -Last 20
    "build exit code: $code"
    if ($code -eq 0) {
        New-Item -ItemType Directory -Force -Path $bin | Out-Null
        Copy-Item target\release\Rebootly.exe, target\release\rebootly-cli.exe $bin -Force
        Get-ChildItem $bin -Filter *.exe | Format-Table Name, Length, LastWriteTime -AutoSize | Out-String
    } else {
        "Build failed: nothing copied."
    }
    Pop-Location
} *>&1 | Tee-Object -FilePath $out
Write-Host "`r`nSaved: $out" -ForegroundColor Green
