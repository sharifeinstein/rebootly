@echo off
rem Rebootly WinPE: finds the Rebootly stick (a drive with .rebootly-root)
rem and runs its start.cmd. Type "rebootly" to run it again.
setlocal
title Rebootly rescue
echo Rebootly rescue (WinPE): looking for the Rebootly stick...
set "STICK="
rem USB drives can get their letter a few seconds after start.
for /l %%t in (1,1,15) do (
    for %%d in (C D E F G H I J K L M N O P Q R S T U V W Y Z) do (
        if not defined STICK if exist "%%d:\.rebootly-root" if exist "%%d:\start.cmd" set "STICK=%%d:"
    )
    if defined STICK goto found
    ping -n 2 127.0.0.1 >nul
)
echo.
echo The Rebootly stick wasn't found (a drive with .rebootly-root and start.cmd).
echo Plug it in and type: rebootly
goto end

:found
echo Found it on %STICK%
call "%STICK%\start.cmd"

:end
echo.
echo Rebootly has closed.
echo   rebootly          start Rebootly again
echo   wpeutil shutdown  switch off
echo   wpeutil reboot    restart
endlocal
