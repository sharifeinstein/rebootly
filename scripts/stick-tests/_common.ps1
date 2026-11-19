# Shared by the test scripts: admin check, results folder, safe native calls.
$ErrorActionPreference = "Continue"
$Stick = Split-Path -Qualifier $PSScriptRoot
$Results = Join-Path $PSScriptRoot "results"
New-Item -ItemType Directory -Force -Path $Results | Out-Null

$id = [Security.Principal.WindowsIdentity]::GetCurrent()
$admin = (New-Object Security.Principal.WindowsPrincipal $id).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $admin) {
    Write-Host "Run this in PowerShell opened with 'Run as administrator'." -ForegroundColor Red
    exit 1
}

function Section($title) { "`r`n===== $title =====" }

# Native tools: stderr merged as text, never as PowerShell errors.
function Native($line) { cmd /c "$line 2>&1" }

function Machine-Info {
    Section "Machine"
    "Computer: $env:COMPUTERNAME   User: $env:USERNAME   Time: $(Get-Date -Format s)"
    $os = Get-CimInstance Win32_OperatingSystem
    "Windows: $($os.Caption) $($os.Version) (build $($os.BuildNumber))"
    Get-CimInstance Win32_VideoController | Format-Table Name, DriverVersion, DriverDate, Status -AutoSize | Out-String -Width 200
}

function Crash-Events {
    Section "Application Error events (last 30 minutes)"
    Get-WinEvent -FilterHashtable @{LogName='Application'; ProviderName='Application Error'; StartTime=(Get-Date).AddMinutes(-30)} -MaxEvents 5 -ErrorAction SilentlyContinue |
        Format-List TimeCreated, Message | Out-String -Width 200
}

# Start Rebootly.exe, watch it for 20 seconds, report what happened.
function Gui-Test($exe) {
    Section "Rebootly.exe start: $exe"
    "WGPU_BACKEND = '$env:WGPU_BACKEND'"
    $trace = Join-Path $env:LOCALAPPDATA "Rebootly\gui-start.txt"
    $crash = Join-Path $env:LOCALAPPDATA "Rebootly\crash.txt"
    Remove-Item $trace, $crash -ErrorAction SilentlyContinue
    $err = Join-Path $Results "gui-stderr.txt"
    $p = Start-Process -FilePath $exe -PassThru -RedirectStandardError $err -RedirectStandardOutput "$err.out"
    if ($p.WaitForExit(20000)) {
        "Exited after start, exit code: $($p.ExitCode)"
    } else {
        $p.Refresh()
        "Still running after 20 s. Responding: $($p.Responding)  Window title: '$($p.MainWindowTitle)'"
        Write-Host ""
        Write-Host "Is the Rebootly window on screen? Look, then close it (or press Enter if there's none)." -ForegroundColor Yellow
        Read-Host "Type yes or no, then Enter" | ForEach-Object { "Owner says window visible: $_" }
        if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue; "Stopped it." }
    }
    Section "stderr"
    Get-Content $err -TotalCount 40 -ErrorAction SilentlyContinue
    Section "gui-start.txt (new builds only)"
    if (Test-Path $trace) { Get-Content $trace } else { "(none)" }
    Section "crash.txt"
    if (Test-Path $crash) { Get-Content $crash -TotalCount 60 } else { "(none)" }
    Remove-Item $err, "$err.out" -ErrorAction SilentlyContinue
}
