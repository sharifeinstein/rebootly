REBOOTLY - move to a new PC, or keep a copy of this one
=======================================================

Rebootly copies your files, app settings and Windows settings to a USB
drive, another disk or a NAS, and puts them back on a new PC. It is free
and open source (Apache License 2.0).

This is a BETA. Keep your old PC (or another copy of your files) until
you have checked everything arrived on the new one.


WHAT'S IN THIS FOLDER
---------------------
  Rebootly.exe              The program. Double-click it.
  rebootly-cli.exe          The same features for a command prompt or a
                            recovery console (run: rebootly-cli --help).
  packs\                    Which app settings Rebootly knows about.
  README.txt                This file.
  LICENSE.txt               Rebootly's licence.
  THIRD-PARTY-LICENSES.html Licences of the components Rebootly uses.

Keep these files together, for example in a folder on the USB drive.


"WINDOWS PROTECTED YOUR PC"
---------------------------
Rebootly isn't code-signed (signing costs money every month, and
Rebootly is free), so Windows may warn you the first time. Click
"More info", then "Run anyway". Only do this if you downloaded Rebootly
from its official page.

Rebootly then asks for administrator rights: it needs them to read
locked files and to restore Windows settings.


BACKING UP THE OLD PC
---------------------
1. Plug in the USB drive (or have the NAS folder ready).
2. Start Rebootly.exe and click "Back up".
3. Choose "This PC", the accounts, and how much to back up:
     Quick  - your folders, app settings and the list of apps
     Full   - everything Rebootly finds, also keys and credentials
              (encrypted with a passphrase you choose)
     Custom - pick yourself, with sizes
4. Choose the drive, check the summary, and click "Start backup".
   You can pause, or stop and continue later.
5. At the end, open the report: it lists what was copied and the
   things to do by hand (for example signing in to apps again).

Before you give the old PC away: deactivate licences (Adobe, Office,
games, ...) and save your BitLocker recovery key if the disk is
encrypted.


THE OLD PC DOESN'T START (OR ONLY ITS DISK IS LEFT)
--------------------------------------------------
Connect the old disk to a working Windows PC (USB adapter), start
Rebootly.exe, click "Back up" and choose "Windows on E:\" (the old
disk). If the disk is locked with BitLocker it is listed as "Unlock E:":
type its 48-digit recovery key (for a Microsoft account it is at
aka.ms/myrecoverykey). Rebootly only reads the old disk.

From a recovery console (WinPE, or a Linux rescue USB), run
rebootly-cli: it finds the Windows on the disks, offers to unlock
BitLocker, and opens every disk read-only. The same steps as commands:
  rebootly-cli volumes            list the disks Windows could be on
  rebootly-cli open <disk>        unlock / open one read-only
  rebootly-cli backup --windows-root <where it opened> --target <dir>
  rebootly-cli close              (Linux) close what "open" opened


RESTORING ON THE NEW PC
-----------------------
1. Sign in to Windows on the new PC with your own account.
2. Plug in the drive and start Rebootly.exe from it.
3. Click "Restore". The newest backup is at the top.
4. Choose whose files to restore and what, then look at the preview.
   "Dry run" goes through everything without changing anything.
5. Click "Restore...". Apps are installed first (winget), then files
   and settings. Tick off the things to do by hand at the end.

Changed your mind? "Undo a restore" puts back everything the restore
replaced (files you changed since then are kept).


WHAT REBOOTLY NEVER DOES
------------------------
- It never changes the old PC: backing up only reads.
- It never reads saved passwords or browser cookies, and never decrypts
  Windows-protected data. Sign in to apps and websites again on the new
  PC (the report reminds you).
- Keys and credentials (SSH keys, certificates, Wi-Fi passwords) are
  only copied into an encrypted file, with a passphrase that is not
  stored anywhere.


NO WARRANTY
-----------
Rebootly is provided "as is", without warranty of any kind (see
LICENSE.txt). Check your backup before you wipe the old PC.
