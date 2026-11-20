Rebootly copies your files, app settings and Windows settings to a USB drive, another disk or a NAS, and puts them back on a new PC. Free and open source (Apache-2.0). **This is a beta:** keep your old PC (or another copy of your files) until you have checked everything arrived on the new one.

## Downloads

| File | What it is |
|---|---|
| `Rebootly-<version>-windows-x64.zip` | The program for Windows 10/11 (x64): `Rebootly.exe` (double-click) and `rebootly-cli.exe` (command prompt, recovery console). Unzip it anywhere, for example onto the USB drive; `README.txt` inside is the user guide. |
| `Rebootly-<version>-stick.zip` | The files for a Ventoy rescue stick: unzip onto the stick's data partition. Includes the Windows exes and a Linux build for `start.sh`. The rescue ISOs are attached separately when available. |
| `*.sha256` | SHA-256 checksums. In PowerShell: `Get-FileHash <file>` and compare. |

## "Windows protected your PC"

Rebootly isn't code-signed yet, so SmartScreen may warn the first time. Click **More info → Run anyway**. Check the download's checksum first if you want to be sure it is the file from this page.

## What's new

<!-- Edit this section in the draft before publishing. -->
