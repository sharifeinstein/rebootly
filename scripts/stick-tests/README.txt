Rebootly tests - run in this order
==================================

Open PowerShell with "Run as administrator" (Start, type PowerShell,
right-click, Run as administrator). Replace E: with the stick's letter
if it differs on that PC. Each script saves its output in E:\tests\results\.

On the Windows 10 laptop:

  1. powershell -ExecutionPolicy Bypass -File E:\tests\1-laptop-gui-dx12.ps1
     Starts Rebootly.exe forced to DirectX 12. If a window opens, look at
     it, close it, and answer the question in PowerShell.

  2. powershell -ExecutionPolicy Bypass -File E:\tests\2-laptop-edge-access.ps1
     Checks why Amouni's Edge files couldn't be read in the live backup.

If step 1 opened the window, stop here. Otherwise:

On your main PC (stick plugged in):

  3. powershell -ExecutionPolicy Bypass -File E:\tests\3-pc-build-and-copy.ps1
     Builds the latest Rebootly and copies it to the stick.

Back on the Windows 10 laptop:

  4. powershell -ExecutionPolicy Bypass -File E:\tests\4-laptop-gui-new-build.ps1

Then plug the stick into your main PC and tell Claude.

Round 2 (2026-11-13: Edge "Access is denied" via the shadow copy):

On your main PC (stick plugged in):

  5. powershell -ExecutionPolicy Bypass -File E:\tests\5-pc-build-test-copy.ps1
     Tests, clippy, release build; copies both exes to E:\bin\win-x64\.

On the Windows 10 laptop (the stick is D: there), Amouni signed out
(Task Manager > Users > right-click Amouni > Sign off):

  6. powershell -ExecutionPolicy Bypass -File D:\tests\6-laptop-edge-vss.ps1
     Short live backup of Amouni's Edge data into Rebootly\ on the stick,
     and the Windows licence check (shows only the last 5 key characters).

Then plug the stick into your main PC and tell Claude.
The scripts only read things and start Rebootly; nothing is changed
except a temporary copy of one Edge file in %TEMP% (deleted again),
the exes copied by step 5 and the backup folder made by step 6.

Round 3 (2026-11-19: the new passphrase prompt):

On your main PC (stick plugged in):

  7. powershell -ExecutionPolicy Bypass -File E:\tests\7-pc-build-test-copy.ps1
     Tests, clippy, release build; copies both exes to E:\bin\win-x64\
     and the small test Windows install to E:\tests\fixture\.

Then on the laptop and in the VMware VM (and on the PC too if you like):

  8. powershell -ExecutionPolicy Bypass -File E:\tests\8-passphrase-prompt.ps1
     (use the stick's letter on that machine, e.g. D: on the laptop)
     Backs up the small test install, NOT this PC, into a temporary folder
     and asks you to type, paste and cancel passphrases (test ones the
     script shows you; never a real one). Answer its y/n questions.
     The results file is named after the computer, so each machine
     keeps its own.

Then plug the stick into your main PC and tell Claude.
Step 8 changes nothing on the machine: its temporary folder in %TEMP% is
deleted at the end, and it empties the clipboard after the paste test.

