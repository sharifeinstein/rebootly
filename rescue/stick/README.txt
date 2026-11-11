REBOOTLY STICK
==============

This stick backs up a Windows PC, also one that no longer starts, and
restores the backup onto a new PC. GUIDE.txt explains Rebootly itself.

On a PC that still starts Windows
---------------------------------
Open the stick in File Explorer and double-click start.cmd (or
bin\win-x64\Rebootly.exe). Windows asks for administrator rights; say yes.

On a PC that doesn't start (or to back up without starting Windows)
-------------------------------------------------------------------
1. Plug in the stick and switch the PC on. Open the boot menu: usually F12,
   F11, F9, Esc or F8 right after switching on (it depends on the maker).
   Pick the USB stick.
2. In the stick's menu pick "Rebootly rescue (Linux)" (works on any PC) or
   "Rebootly rescue (WinPE)" (if you built one).
3. Rebootly starts by itself. Pick the Windows to rescue, then where the
   backup goes: this stick (its Rebootly folder) or another USB drive.
   A BitLocker disk asks for its 48-digit recovery key (aka.ms/myrecoverykey)
   or its password.
4. When it's done, leave Rebootly (Esc). It flushes and unmounts the drives;
   then press p to switch the PC off.

Nothing on the PC's own disks is changed: they are only read.

Secure Boot
-----------
If the PC says "Verification failed" or doesn't show the stick's menu:
 - Either enrol Ventoy's key once: in the blue screen pick "Enroll key from
   disk", choose VTOYEFI, then ENROLL_THIS_KEY_IN_MOKMANAGER.cer, confirm and
   reboot.
 - Or switch Secure Boot off in the PC's firmware settings (and back on
   afterwards: a BitLocker PC then asks for its recovery key once).

In the rescue system
--------------------
After Rebootly closes, a small menu offers: start again, keyboard layout,
network (for a NAS: then "s" for a shell and
  mount -t cifs //NAS/share /media/nas -o username=NAME
), shell, restart, power off. On the shell:
  rebootly-cli volumes             list the disks
  rebootly-cli target /dev/sdb1    use a USB drive for writing
  rebootly-cli close               flush and unmount everything

Folders on this stick
---------------------
  Rebootly\      your backups (one folder per PC and time)
  bin\           Rebootly for Windows (x64, ARM64) and Linux
  iso\           the rescue systems the stick starts
  packs\         app settings Rebootly knows (Chrome, VS Code, ...)
  ventoy\        the stick's menu settings
  .rebootly-root how Rebootly recognises its stick; leave it
