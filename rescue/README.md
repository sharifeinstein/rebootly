# Rebootly rescue stick (SPEC §8)

A USB stick that starts a PC without its own Windows and runs Rebootly, so
a PC that no longer boots can still be backed up. It is a
[Ventoy](https://www.ventoy.net) stick with Rebootly's files on it:

```
VENTOY (exFAT data partition)
├─ .rebootly-root                 marker: how Rebootly finds its stick
├─ start.cmd, start.sh            launchers (Windows/WinPE, Linux)
├─ README.txt, GUIDE.txt          for the person using the stick
├─ bin/win-x64/  Rebootly.exe, rebootly-cli.exe
├─ bin/win-arm64/ (optional)
├─ bin/linux-x64/rebootly-cli     static (musl), runs on any x86-64 Linux
├─ packs/
├─ iso/rebootly-rescue-x86_64.iso (+ .sources.txt)   built by alpine/build.sh
├─ iso/rebootly-winpe-amd64.iso   built by you: winpe/build.ps1 (not redistributable)
├─ ventoy/ventoy.json             menu names; search only iso/; VTOY_LINUX_REMOUNT
└─ Rebootly/                      backups made onto the stick
```

## Making a stick

1. **Ventoy**: download it from ventoy.net, run `Ventoy2Disk.exe`, pick
   the stick, *Install* (this erases the stick). Leave the data partition
   exFAT (or choose NTFS under Option → Partition Configuration); not
   FAT32 (4 GiB file limit).
2. **Rescue ISOs** (once per Rebootly release that changes them):
   - Linux (works on every PC): `sh rescue/alpine/build.sh` on Alpine, or
     `sh rescue/alpine/build-in-docker.sh` (WSL/Linux with Docker), or
     `.\rescue\alpine\build-in-docker.ps1` (Docker Desktop). Writes
     `dist/rebootly-rescue-x86_64.iso` and `dist/linux-x64/rebootly-cli`.
   - WinPE (optional): install the Windows ADK and its WinPE add-on, then
     in an elevated PowerShell `.\rescue\winpe\build.ps1`. Writes
     `dist\rebootly-winpe-amd64.iso`.
3. **Build Rebootly** (`cargo build --release -p rebootly-gui -p rebootly-cli`)
   and run `.\rescue\make-stick.ps1 -Drive E:` (the stick's letter). It
   copies everything above that has been built; run it again after a
   Rebootly update. `scripts\package-release.ps1` builds the same thing
   as `dist\Rebootly-<version>-stick.zip`.

## How it starts

- **Linux rescue** (Alpine): `tty1` runs `rebootly-start` instead of a
  login. It mounts the stick read-write (`rebootly-cli target --stick`),
  runs the stick's `start.sh` (so a newer Rebootly on the stick needs no
  new ISO; the ISO's own copy is the fallback), then `rebootly-cli close`
  and a menu (again, keyboard layout, network, shell, restart, power
  off). Shells on Alt+F2 and Alt+F3. Network is off until asked for.
- **WinPE**: `startnet.cmd` runs `wpeinit`, then `rebootly.cmd` looks for
  `.rebootly-root` on C:–Z: (for up to ~30 s) and calls the stick's
  `start.cmd`, which runs `rebootly-cli tui` (WinPE has no GPU for the
  window). Type `rebootly` to start it again.
- **Windows**: `start.cmd` opens `Rebootly.exe` with the stick's packs.

The rescue system only writes to backup targets: the stick, or a USB
drive picked on the "where to" screen (mounted under `/media`). A disk
with Windows on it is never mounted for writing.

## Known limits / to check on real hardware

- The Ventoy data partition is held by Ventoy's device mapping after a
  Linux boot; `VTOY_LINUX_REMOUNT` (in `ventoy.json`, Ventoy ≥ 1.0.64,
  marked experimental by Ventoy) makes it mountable. If it fails, the
  stick isn't offered and a second USB drive must be the target.
- The Alpine image has no `dislocker` (only in Alpine's testing
  repository): BitLocker is opened with `cryptsetup` (BITLK) only.
- x86-64 only for now; an aarch64 image builds the same way on an ARM64
  Alpine host.
- Secure Boot: Ventoy's key must be enrolled once (README.txt), or Secure
  Boot turned off.
- The Alpine ISO contains GPL software: ship `rebootly-rescue-*.sources.txt`
  with it (make-stick does).

## Testing in a VM

Hyper-V: a Generation 2 VM with Secure Boot off (or set to the "Microsoft
UEFI Certificate Authority" template), the ISO in its DVD drive, the test
Windows disk (a copy) as a second disk, and a small exFAT or NTFS VHDX
with `.rebootly-root`, `start.sh`/`start.cmd`, `bin/` and `packs/` at its
root to play the stick. That checks the rescue system and Rebootly in it;
booting through Ventoy (menu, `VTOY_LINUX_REMOUNT`) is easiest to check
on a real stick.
