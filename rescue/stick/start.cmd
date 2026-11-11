@echo off
rem Rebootly stick launcher for Windows and WinPE. On Windows it opens the
rem Rebootly window (it asks for administrator rights); in WinPE the text
rem screens. Extra arguments go to Rebootly.
setlocal
set "HERE=%~dp0"

set "ARCH=x64"
if /i "%PROCESSOR_ARCHITECTURE%"=="ARM64" set "ARCH=arm64"
if /i "%PROCESSOR_ARCHITEW6432%"=="ARM64" set "ARCH=arm64"

reg query "HKLM\SYSTEM\CurrentControlSet\Control\MiniNT" >nul 2>&1
if not errorlevel 1 goto winpe

rem Windows on ARM runs the x64 build too, if there is no ARM64 one.
if not exist "%HERE%bin\win-%ARCH%\Rebootly.exe" set "ARCH=x64"
if not exist "%HERE%bin\win-%ARCH%\Rebootly.exe" (
    echo This stick has no Rebootly for Windows ^(bin\win-x64\Rebootly.exe is missing^).
    pause
    exit /b 1
)
start "" "%HERE%bin\win-%ARCH%\Rebootly.exe" --packs-dir "%HERE%packs" %*
exit /b 0

:winpe
rem WinPE has no x64 emulation on ARM: the build must match.
if not exist "%HERE%bin\win-%ARCH%\rebootly-cli.exe" (
    echo This stick has no Rebootly for WinPE on %ARCH% ^(bin\win-%ARCH%\rebootly-cli.exe is missing^).
    exit /b 1
)
mode con: cols=120 lines=40 >nul 2>&1
"%HERE%bin\win-%ARCH%\rebootly-cli.exe" tui --packs-dir "%HERE%packs" %*
exit /b %errorlevel%
