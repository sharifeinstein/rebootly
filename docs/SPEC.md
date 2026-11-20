# SPEC — Portable PC Migration & Backup Tool (name: **Rebootly**; binaries `Rebootly.exe` (GUI) and `rebootly-cli` (commands, text screens))

> Source of truth for the project. Keep in the repo at `docs/SPEC.md`.
> Read only the sections relevant to the current milestone.

---

## 0. Why this exists

Microsoft removed Windows 11's local PC-to-PC Migration (Oct 2026 Patch Tuesday, KB5127648). The remaining guided path, Windows Backup, syncs only five user folders through OneDrive (5 GB free) and does not move apps. This tool replaces that with a **local, unlimited, settings-aware** migration and backup that runs from a USB stick: in normal Windows, in WinPE, or in a small Linux recovery environment (dead PC, BitLocker key available).

**Not a random dump of user data.** It migrates what makes a PC *feel* like the old one: folders, app settings, shell and personalization settings, and app inventory. It also tells the user exactly what could not be migrated.

---

## 1. Hard requirements

1. A single self-contained executable per platform. Nothing to install, no runtime.
2. Runs in three environments: **live Windows 10/11**, **WinPE**, and **Linux live/recovery** (x86_64). ARM64 Windows is also targeted.
3. Backup targets: USB/local disk, SMB/NAS share, any mounted path. SFTP later.
4. Backup format: **plain folders plus `manifest.json`**. No proprietary container. Optional encryption (see §9).
5. Extensible through **app packs** (YAML) and ad-hoc custom folders chosen by the user.
6. **Never write to the source disk** in offline mode (read-only mount).
7. Honest reporting: anything that cannot migrate goes on a printable "Manual steps" checklist.
8. The tool **never decrypts credentials** (no DPAPI decryption, no reading browser password databases). This is a security and antivirus-heuristics design principle.

---

## 2. Tech decisions (settled)

- **Language: Rust.** Reasons: static single binaries for Windows (static CRT) and Linux (musl); Microsoft's `windows` crate for Win32/COM; memory safety for a tool that touches every user file; C#/.NET is unusable in WinPE.
- **UI:** a Windows GUI (`Rebootly.exe`, egui/eframe with the wgpu renderer) for live Windows, and the `ratatui` text screens (`rebootly-cli` with no command) for consoles, WinPE, a Linux TTY and SSH. Both run on the same core and the shared machine layer (`rebootly-host`); every action also has a `rebootly-cli` command.
- **Registry hives:** a pure-Rust offline hive parser, used in **all** modes. **Our own** read-only parser (`rebootly-core::hive`); `nt-hive` was rejected because it is GPL-2.0-or-later (decided M1, see PROGRESS.md). It is cross-platform and also covers users who aren't logged in. In live mode the Win32 API is a cross-check only.
- **Serialization:** `serde` with YAML for packs and JSON for the manifest.
- **Targets:** `x86_64-pc-windows-msvc` (+crt-static), `aarch64-pc-windows-msvc`, `x86_64-unknown-linux-musl`.
- **License:** Apache-2.0 (decided 2026-11-09). Track dependency licenses with `cargo-deny` (`deny.toml`, same allow list as `about.toml`; also RustSec advisories, wildcard/unknown sources; Windows x64/arm64 graph, and the Linux CLI with `--target x86_64-unknown-linux-musl --exclude rebootly-gui`). Crates are `publish = false`.

### Workspace layout

```
rebootly/
├─ crates/
│  ├─ rebootly-core/    # engine, Source trait, packs, manifest, hive parser, resolver, copy/verify, report
│  ├─ rebootly-win/     # VSS, KnownFolder API, live registry, SeBackupPrivilege, manage-bde, winget, netsh
│  ├─ rebootly-linux/   # volume discovery, BitLocker unlock (cryptsetup/dislocker), ro mount
│  ├─ rebootly-cli/     # thin CLI (scripting, tests, IT techs)
│  ├─ rebootly-host/    # machine layer shared by the front ends
│  ├─ rebootly-tui/     # text screens (consoles, WinPE, Linux)
│  └─ rebootly-gui/     # Windows GUI (egui), binary Rebootly.exe
├─ packs/              # built-in app packs (YAML), shipped beside the binary
├─ rescue/             # Ventoy layout, Alpine build script, WinPE build script
├─ tests/fixtures/     # sample hives, fake profile trees
└─ docs/  SPEC.md  PROGRESS.md
```

---

## 3. Core architecture

### 3.1 Environment detection
- Live Windows: normal case.
- WinPE: the key `HKLM\SYSTEM\CurrentControlSet\Control\MiniNT` exists.
- Linux: compile-time `cfg`.

### 3.2 `Source` trait
Implementations: `LiveWindows`, `OfflineVolume` (WinPE or Linux, Windows installed on a mounted volume).
The trait provides: enumerate profiles, resolve folders, open file for read (with backup semantics), read hive keys, and list installed apps. Packs, manifest and copy engine never know which implementation they are using.
Implementation (M4): `OfflineVolume` reads through a `VolumeAccess` strategy (how hives and files are opened, how paths are mapped). `LiveWindows` is `OfflineVolume` with the live strategy, so the resolution logic is the same in every mode. Live mode **requires an elevated process** (SeBackupPrivilege). Hives that are loaded in the registry (SOFTWARE, SYSTEM, signed-in users' NTUSER.DAT/UsrClass.dat) are saved with `RegSaveKeyExW` into a private temp folder, parsed from there, and kept in the backup as the raw hive (clean, so no logs); hives that aren't loaded are read from their files with backup semantics. Live paths map every drive letter as-is. Paths are compared case-insensitively wherever planned roots are matched (NTFS is case-insensitive; live paths keep the registry's casing).

### 3.3 Locating the Windows install (offline)
Scan every mounted volume for `\Windows\System32\config\SOFTWARE`. **Never assume C:** (WinPE assigns other letters). If more than one install is found, let the user choose.
Implementation (M7): volumes are listed by the platform layer (`rebootly-core::volume::Volume`: device, label, size, filesystem, mounted/locked/unmounted, partition identity). Linux: `lsblk -J -l`, gaps filled with `blkid -p` (no udev on minimal rescue systems); WinPE/Windows: drive letters, a locked BitLocker volume recognised by `GetVolumeInformationW` failing with `FVE_E_LOCKED_VOLUME` (fallback: its `-FVE-FS-` boot sector). The offline Windows' **other drive letters** (D:, …) come from its `SYSTEM\MountedDevices` (MBR signature + offset, or GPT partition GUID) and are matched to mounted volumes by that identity (Linux: PARTUUID/PTUUID + partition start; WinPE: its own `MountedDevices`), never by the letter WinPE assigned. A backup target on one of those drives gets a warning.

### 3.4 Profile and folder resolution
- Profiles come from `SOFTWARE\Microsoft\Windows NT\CurrentVersion\ProfileList`: SID → `ProfileImagePath`. Skip system SIDs (S-1-5-18/19/20). Label local, Microsoft and Entra ID/domain accounts.
- Folders: each user's `NTUSER.DAT` → `Software\Microsoft\Windows\CurrentVersion\Explorer\User Shell Folders`. **Expand `%USERPROFILE%` and other variables manually**, using that user's profile path. Fall back to `Shell Folders`, then to the defaults.
- **Dirty hives:** after a crash, `NTUSER.DAT` may have unapplied `.LOG1/.LOG2` transaction logs. Detect this and warn. Support log replay if the parser can; otherwise read the base hive and flag it in the report. Always copy the hive and its logs into the backup as raw files.
- In live mode, cross-check against `SHGetKnownFolderPath` for the current user.
- The manifest stores **KNOWNFOLDERID → resolved source path** for every folder.

### 3.5 Copy engine
- **Never follow reparse points.** Junctions such as `Application Data`, `My Documents` and `Local Settings` cause infinite loops. Symlinks and junctions are recorded in the manifest, not traversed.
- OneDrive Files On-Demand placeholders (`RECALL_ON_DATA_ACCESS`/`OFFLINE` attributes, cloud reparse tag): user policy is skip, hydrate (live only), or skip the whole folder. Implemented (M4): `--cloud-files skip` (default; excluded category + Manual step) and `--cloud-files hydrate` (live only; pre-flight warns how much will be downloaded onto the source disk). Skip-whole-folder is not implemented. **Downloaded** OneDrive files keep a cloud reparse point; where OneDrive's filter driver isn't running (WinPE, VSS shadow copies) every open fails with error 1920, so such a file is reopened with `FILE_FLAG_OPEN_REPARSE_POINT` and its own data stream is read, but only when its attributes show the data is all on the disk (reparse point, not sparse, no recall/offline flags); otherwise it fails as online-only, never read as zeros. Linux: attributes come from the `system.ntfs_attrib_be` xattr (ntfs3 and ntfs-3g), so placeholders are skipped offline too; ntfs3 reads downloaded OneDrive files as plain files; ntfs-3g can't (it shows them as links to `unsupported reparse tag 0x…`), so those are reported as `UnsupportedReparse` issues (pre-flight warning: mount with ntfs3).
- Live mode: `SeBackupPrivilege` plus `FILE_FLAG_BACKUP_SEMANTICS`, to read other users' profiles past their ACLs (admin required). Locked files are read from a **VSS snapshot** when elevated; otherwise prompt "close app X". Implemented (M4): a sharing or lock violation switches that file to a shadow copy at once (no backoff); the shadow is made lazily, once per volume, through WMI `Win32_ShadowCopy.Create` (ClientAccessible) run by PowerShell, and deleted when the job ends. IDs are kept in `%TEMP%\rebootly-vss-<pid>.txt` so the next live run deletes shadows left by a killed run. "Access is denied" (os error 5, even with backup semantics: usually an antivirus filter driver, which often isn't attached to shadow copies) when opening or reading the source switches to the shadow copy the same way (2026-11-13); errors writing the destination never do. If no shadow can be made, the file fails at once with "close the program and run --resume" (for access denied: that error, so the report's os-error-5 hint shows). The manifest marks such files `snapshot: true`.
- Long paths (`\\?\`), timestamps and attributes preserved. ACLs optional (off by default). Alternate data streams: drop `Zone.Identifier` by default (configurable); record others. Implemented (M4, live only): other streams are listed from a backup-semantics handle and stored as separate files `streams/<dest>/<name>` (targets like exFAT/NAS can't hold streams), recorded per file in the manifest; `--keep-zone-identifier` keeps that one too; streams over 64 MiB are listed, not stored. Offline mounts don't expose streams.
- **Priority order:** Desktop, Documents, Pictures → app packs/settings → Music, Videos → Downloads → custom folders. The user can reorder.
- **Failing-disk tolerance:** retry a read N times with backoff, then log and continue. Never abort the whole job over one file. If SMART data looks bad (when readable), suggest imaging with `ddrescue` first.
- **Resumable:** the job state file on the target lets the copy resume after a NAS dropout or cancel. Pause and resume are supported.
- Parallel, bounded I/O. A SHA-256 hash per file goes into the manifest (computed while streaming).
- **Pre-flight:** size per category, target free space, **FAT32 4 GB limit detection** (warn, suggest exFAT/NTFS), path-length and invalid-character checks for SMB/Linux targets, case-collision check for case-sensitive NAS. Live: a warning naming other users still signed in (WTS sessions other than Rebootly's own; their running apps keep files locked). The target's filesystem type comes from /proc/self/mountinfo on Linux and `GetVolumeInformationW` on Windows.
- Live mode: prevent sleep during the job (`SetThreadExecutionState`). Warn if a laptop is on battery.

### 3.6 Exclusions (always)
`pagefile.sys`, `hiberfil.sys`, `swapfile.sys`, `$Recycle.Bin`, `System Volume Information`, `%TEMP%`, `*.ost`, `Thumbs.db`, `INetCache`, `CrashDumps`, `WER`, every browser cache dir (`Cache`, `Code Cache`, `GPUCache`, `Service Worker\CacheStorage`, `ShaderCache`), `Packages\*\AC`, `Packages\*\TempState`. `NTUSER.DAT*` and `UsrClass.dat*` are copied raw into `_system/hives/`, never restored over a live profile. Optional: `node_modules`, `.git/objects`. Keep `desktop.ini`.
**Never copy** browser `Login Data`, `Cookies`, `Local State` encryption keys, or Credential Manager vaults (they can't decrypt elsewhere, and copying them looks like infostealer behaviour).
Implemented list (M2): Chromium `Login Data*`, `Cookies`, `Local State`; Firefox `logins.json`, `key3/key4.db`, `cookies.sqlite`, `signons.sqlite`; `Microsoft\Credentials`, `Microsoft\Vault`, and the DPAPI `Microsoft\Protect` folder. Their SQLite sidecars (`-journal`, `-wal`, `-shm`) count as the same secret (M5). The thunderbird pack also leaves out the OpenPGP secret keys (`secring.gpg`, `encrypted-openpgp-passphrase.txt`; unusable without `key4.db`) (M9). AppData-only rules (caches, secrets) match only under `AppData\`, so a user's own `Documents\Cache` is still copied.

---

## 4. What to migrate (the folder map)

### Tier 1 — user folders (resolved via §3.4)
Desktop, Documents, Downloads (optional), Pictures, Music, Videos, Favorites, Links, Contacts, Saved Games, Searches, 3D Objects (Win10, if not empty), `C:\Users\Public\*` (ask), and non-system folders at a drive root (detected and offered, e.g. `C:\Projects`).

### Tier 2 — AppData (curated; never the whole of Roaming or Local)
- Shell: `%APPDATA%\Microsoft\Windows\Start Menu\`; taskbar pins at `%APPDATA%\Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar\` plus the registry key `Taskband`; Quick Access/jump lists at `%APPDATA%\Microsoft\Windows\Recent\AutomaticDestinations\` (`f01b4d95cf55d32a.automaticDestinations-ms`); `Themes\` (TranscodedWallpaper); `SendTo\`.
- Office: `%APPDATA%\Microsoft\Templates\` (Normal.dotm), `Signatures\`, `UProof\` (CUSTOM.DIC), Outlook PSTs (found via registry and `Documents\Outlook Files`, `%LOCALAPPDATA%\Microsoft\Outlook\`). PST only, no OST.
- Fonts: `%LOCALAPPDATA%\Microsoft\Windows\Fonts\`, plus non-stock fonts in `C:\Windows\Fonts` (compare with a stock list; needs reinstall).
- Sticky Notes: `%LOCALAPPDATA%\Packages\Microsoft.MicrosoftStickyNotes_8wekyb3d8bbwe\LocalState\plum.sqlite`
- Browsers (profiles minus caches and minus the secrets in §3.6): Chrome `%LOCALAPPDATA%\Google\Chrome\User Data\`, Edge `%LOCALAPPDATA%\Microsoft\Edge\User Data\`, Brave, Vivaldi, Opera, Firefox `%APPDATA%\Mozilla\Firefox\Profiles\` plus `profiles.ini` (Firefox: skip `logins.json`/`key4.db` too).
- Mail: Thunderbird `%APPDATA%\Thunderbird\Profiles\`. New Outlook / Windows Mail is cloud-based: report only.
- Per-user installed apps: `%LOCALAPPDATA%\Programs\` (offer; portable apps work, others need reinstall).
- App packs (§6) cover everything else.

### Tier 3 — registry (HKCU allowlist only, never the whole hive)
`Control Panel\` (mouse, keyboard, desktop, accessibility, international), `Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced`, `\Taskband`, `\HideDesktopIcons`, `...\Themes\Personalize`, `Keyboard Layout\`, `Control Panel\International\User Profile`, `Network\` (mapped drives), `Environment` (user env vars, reviewed on restore), selected Office keys (AutoCorrect, MRU), plus per-pack keys. Read from the offline hive in all modes; saved as `.reg` text in the backup.

### Tier 4 — system-level (live: commands; offline: from hives/files)
| Item | Live capture | Offline capture | Restore |
|---|---|---|---|
| Installed apps | `winget export` + Uninstall keys | Uninstall keys from the SOFTWARE hive (HKLM + WOW6432Node) and per-user from NTUSER | `winget import` + checklist for non-winget apps |
| Wi-Fi | `netsh wlan export profile key=clear` (**encrypted bundle**) | Copy XMLs from `ProgramData\Microsoft\Wlansvc\Profiles\Interfaces\` (keys **not** recoverable → checklist) | `netsh wlan add profile` |
| Printers | `Get-Printer` / `printbrm -b` | List from the SYSTEM hive | Reinstall checklist / `printbrm -r` |
| Default apps | `dism /online /Export-DefaultAppAssociations` | — | Report only (Win11 blocks silent set) |
| Hosts file | copy | copy | diff and merge |
| User certs | `certutil -user -exportPFX` if exportable | — | import |
| VPN | `%APPDATA%\Microsoft\Network\Connections\Pbk\rasphone.pbk` (+ all-users `%ProgramData%\…\Pbk`) | same | copy |
| Scheduled tasks (non-Microsoft) | `schtasks /query /xml` | XMLs from `Windows\System32\Tasks` | `schtasks /create /xml` |
| Power plan | `powercfg /export` | — | `powercfg /import` |
| Drivers | `pnputil /export-driver` (reference only) | — | never auto-install |
| Windows licence | `DigitalProductId` / `DigitalProductId4` from SOFTWARE (same code) | same | checklist only (key in the bundle) |

Implemented (M4, part 1): `system/hosts`; scheduled tasks outside `Tasks\Microsoft` → `system/tasks/`; Wi-Fi profile XMLs (offline) → the sensitive bundle only (`system/wifi/`), and the saved network names (plain text in the XMLs; only the key is protected) are listed in the "Wi-Fi networks" checklist step in every mode, so a Quick backup also says which networks to re-join; `system/apps.json` (machine-wide Uninstall entries). Live only: `system/winget.json` (`winget export --include-versions`), `system/printers.json` (`Get-Printer`), and with `--sensitive` `netsh wlan export profile key=clear` into the bundle (`system/wifi-export/`; the clear-text XMLs exist only in a private temp folder during the job). Each command has a 180 s timeout; failures become warnings. `--no-system` leaves all of this out.
Implemented (M4, part 2): per-user `rasphone.pbk` via the `windows-shell` pack (v2), all-users one → `system/vpn/rasphone.pbk` (both modes; it holds no passwords). Live only: `system/power/active-scheme.pow` (`powercfg /export` of the active scheme), `system/default-apps.xml` (`dism /Export-DefaultAppAssociations`), `system/drivers.txt` (`pnputil /enum-drivers`, reference only). User certificates: with `--sensitive`, `Cert:\CurrentUser\My` of the user running the tool is exported with `Export-PfxCertificate` (instead of certutil, which exports one certificate per call and takes the password on its command line) into the bundle as `system/certs/<thumbprint>.pfx` (`.cer` when there's no key or it isn't exportable, plus a warning), protected by a random password stored next to them as `system/certs/PFX-PASSWORD.txt` inside the bundle. Other users' certificates need their own sign-in.

Windows licence (2026-11-12): read from the SOFTWARE hive in every mode (`rebootly-core::licence`): the key decoded from `DigitalProductId` (Windows 8+ layout; `SoftwareProtectionPlatform\BackupProductKeyDefault` as fallback), the channel (`Retail`, `OEM:DM`, `OEM:…`, `Volume:MAK`, `Volume:GVLK`) found by value in `DigitalProductId4`, and the edition. Generic default keys mean a digital licence (nothing to copy); KMS client keys or `Volume:GVLK` mean an organisation activates it. A checklist step in every mode says which kind it is, the key's last group and what to do on the new PC (Change product key; the Activation troubleshooter with the linked Microsoft account; OEM and firmware keys stay with the old PC; same edition needed). The full key goes only into the encrypted bundle (`system/licence/windows-key.txt`), and only for kinds worth keeping (retail, OEM, firmware, MAK, unknown). `rebootly-cli inspect` shows the kind and last group. The firmware (`MSDM`) key isn't read: an offline backup can't tell whose PC's firmware it would be, and an `OEM:DM` install already holds that key.

### Tier 5 — cannot migrate → "Manual steps" checklist
Browser/app passwords (prompt for a CSV export in live mode, **import then securely delete**; or use browser sync), Credential Manager entries (`cmdkey /list` names only), software licenses/activation (list + **"deactivate on the old PC now"** prompt in live mode: Adobe, iTunes, Office, etc.), Store app data (except known packs), Windows Hello/TPM, BitLocker (remind to save the recovery key), offline Wi-Fi keys.

---

## 5. Restore

- **Restore runs in live Windows only** (it needs HKCU, winget and netsh). File-only restore may be allowed from offline environments later; never touch the registry offline.
- **Phases, in order:** (1) pre-flight and mapping → (2) install apps (`winget import`) → (3) user waits or chooses "apps installed" → (4) files → (5) app data and packs (**with the app closed**; detect running processes) → (6) registry `.reg` import (allowlisted) → (7) Wi-Fi, printers, tasks, etc. → (8) Explorer restart → (9) verify (re-hash) → (10) Manual steps checklist.
- **Remapping:** old user → new user (different username/SID), drive letters, OneDrive on or off, KNOWNFOLDERID → new path.
- **Path rewriting** inside text configs that packs declare (`rewrite: true`): replace `C:\Users\olduser` with the new profile path (VS Code settings, `.gitconfig`, Steam `libraryfolders.vdf`, etc.). Back up the original first.
- **Conflict policy** (global + per category): skip / overwrite if newer / keep both (`name (migrated).ext`) / ask. **Dry run** shows every action before anything is written.
- **Undo:** every overwritten file is saved first to `%USERPROFILE%\.rebootly-undo\<job>\`, so a restore can be rolled back.
- **Version gating:** packs and registry items can be marked `os: [win10, win11]` and `min_app_version`. Win10 → Win11 skips keys that don't apply.

Implemented (M5): `rebootly-cli restore <BACKUP>` and `rebootly-cli undo [UNDO_DIR] [--list]`. One run restores one backup user (`--user NAME`; picked automatically when there is only one, or one with the same name) into the user running Rebootly, whose known-folder paths come from `SHGetKnownFolderPath` (so OneDrive folder backup on the new PC is followed). Needs an elevated prompt (except `--dry-run`). Mapping: `users/<u>/folders/<KF>` → the new path of that known folder; pack files → the pack template expanded for the new user (a destination outside the profile is allowed only when the template is a path of the pack installed in this Rebootly); `custom/<label>` → the old path with old profile → new profile, old `Users\Public` → new, then `--map-drive D=E`; never into `Windows` or `Program Files`. Raw hives, `drivers.txt` and `apps.json` are reference only. Conflicts: identical files (size + SHA-256) are always skipped; default policy is keep-both for folders and custom folders, overwrite for packs and system files (`--conflict`, `--conflict-for folders=skip,packs=ask`); `overwrite` was added to the four policies because a freshly installed app's default settings are newer than the old ones. A `desktop.ini` that already exists is kept, and so is an existing shortcut (`.lnk`/`.url`: never replaced or duplicated, the new PC's one points to the right places). Files named after the old account's SID are skipped, and so are binary pack files with the old profile path inside when the profile path changes (Quick Access pins, shortcuts): they can't be rewritten. Hosts and `rasphone.pbk` (both) are merged: missing entries/connections are appended. Files are written as `.rbpart`, checked against the backup's SHA-256, renamed into place, mtime kept, streams written back, hidden/system attributes set; read back and re-hashed in the verify phase. Path rewriting (`rewrite: true`): `C:\Users\old` in its `\`, `\\` and `/` spellings, whole path components only, in UTF-8 (with or without BOM) and UTF-16LE text up to 16 MiB. Registry: each pack's `.reg` must pass the allowlist of that pack *as installed* (only `HKEY_CURRENT_USER` keys under its `registry:` entries, no key or value deletions); profile paths in string values are rewritten; `HKCU\Environment` and `HKCU\Network` are flagged for review in the plan. Before importing, each top-level key is exported to the undo folder (absent keys are recorded, and deleted on undo). Phase 3 is a prompt after `winget import` (`--apps install|skip|only`). Packs whose `requires_closed` process is running are skipped with a "close it and run again" note. System: Wi-Fi from `system/wifi-export/` (bundle) via `netsh wlan add profile`; offline Wi-Fi XMLs only become a "re-join" checklist; power plan: if a scheme with the old active plan's GUID exists (built-in plans share GUIDs), it is made active, otherwise the exported plan is imported and made active; certificates imported into `Cert:\CurrentUser\My` (PFX password read from the bundle file by PowerShell, never on a command line), only when restoring the user who ran the backup (`system/certs/OWNER.txt` in the bundle holds their SID; without it, nothing is imported and a manual step explains why); scheduled tasks only with `--tasks` (apps usually re-create their own); printers, default apps and drivers are manual steps. Existing Wi-Fi profiles, certificates and tasks are left alone. Explorer is restarted only after a registry import or a `windows-shell` pack change. The sensitive bundle is opened with `--sensitive` into a private temp folder that is deleted afterwards. Undo: `%USERPROFILE%\.rebootly-undo\<job>_<time>\` holds `journal.jsonl` (written and fsynced *before* each change), the replaced files (`files/`), exported keys (`reg/`) and `report.txt`; undo walks the journal backwards, leaves files changed since the restore alone (unless `--force`), removes created folders if empty, deletes imported Wi-Fi profiles, certificates, tasks and the power plan, and marks the run `undone.txt`. `--simulate-root DIR` restores files under `DIR/C/…` (system steps only listed) for previews and tests off Windows. Not yet: `min_app_version`, offline file-only restore, read-only attributes, ACLs.

---

## 6. App packs

```yaml
schema: 1
id: vscode
name: Visual Studio Code
detect:
  any:
    - path: "%LOCALAPPDATA%\\Programs\\Microsoft VS Code\\Code.exe"
    - uninstall_name: "Microsoft Visual Studio Code*"
winget_id: Microsoft.VisualStudioCode
paths:
  - src: "%APPDATA%\\Code\\User"
    include: ["settings.json", "keybindings.json", "snippets/**"]
    rewrite: true            # rewrite old profile paths on restore
  - src: "%USERPROFILE%\\.vscode\\extensions"
    mode: list_only          # record the list, reinstall rather than copy
registry: []
requires_closed: ["Code.exe"]
os: [win10, win11]
notes: "Extensions reinstalled via `code --install-extension`."
post_restore:
  - run: "code --install-extension {each:extensions}"
    live_only: true
```

- Packs live in `packs/` next to the binary. They can be updated without rebuilding, and `schema` is versioned.
- "Add custom folder" in the UI writes an ad-hoc pack into the job file.
- "Import pack" accepts a YAML file. Validate it against the schema and show a clear error.
- **Initial built-in packs (v1):** Chrome, Edge, Firefox, Brave, Thunderbird, Outlook (PST + signatures), Office (templates, dictionary), VS Code, JetBrains, Notepad++, Windows Terminal, PowerShell profiles, Git/SSH/dev dotfolders (`.gitconfig`, `.ssh`, `.aws`, `.azure`, `.kube`, `.docker/config.json`, `.npmrc`, `.config`), Sticky Notes, FileZilla, PuTTY, WinSCP, OBS, Adobe (presets), Steam (userdata + library re-link), Minecraft, `Documents\My Games`, QuickBooks (example of a business app).
- `.ssh` private keys and cloud credential files go in the **sensitive bundle** (§9) automatically when the user gives a passphrase (`--sensitive`).
- Schema details (M3): optional `version` (pack content version, default 1); per-path `exclude` globs and `sensitive: true`; detect conditions are one-key maps: `path`, `uninstall_name` (glob on DisplayName, machine and user Uninstall keys), `registry` (HKCU key exists), `always`. Path templates may use `%KF:<Folder>%` for a resolved known folder (e.g. `%KF:Documents%\\PowerShell`, which follows OneDrive redirection). `registry` keys must be HKCU and are exported recursively to `users/<u>/registry/<pack-id>.reg` (UTF-16LE, regedit 5.00). Copied paths go to `users/<u>/packs/<id>/<leaf>/`. Built-in packs are compiled into the binary; YAML files in `packs/` with the same `id` replace them. A pack detected only by a machine-wide install, with nothing of that user's on disk, is dropped for that user. `sensitive` paths go only into `sensitive.age` (status `encrypted`); without a passphrase they are not copied at all (status `sensitive_not_copied`, plus a Manual step). Either way a parent folder never walks into them, so they are never copied in plain. **Saved passwords next to non-secret settings (M9):** a path's `redact:` entries (`file` relative to the path, or omitted for a file path, `*`/`?` allowed within a segment; `xml:` element selectors `Name` / `Name[attr=glob]` and `ini:` key names are removed; `json:` key names get their string value blanked, so the file stays valid JSON) strip them from the plain copy (written as a generated file; a file with none is copied as usual), and the pack-level `secret_values:` (registry value names, case-insensitive) are left out of the plain `.reg`. With a passphrase the untouched files and the full `.reg` also go into `sensitive.age` at the same paths, and restore with the bundle open uses those instead of the plain ones; without one, a Manual step says which apps' passwords must be entered again. Manifest: `PackRecord.secrets` (status, files, registry value count). Used by filezilla, putty, winscp, obs.
- Work-app packs (M9): **outlook** (classic) copies signatures and the PSTs in `%LOCALAPPDATA%\Microsoft\Outlook` (PSTs in `Documents` come with Documents; `.ost` never), not account settings; **office** copies templates, custom dictionaries (`UProof`), AutoCorrect (`*.acl`), ribbon/QAT (`*.officeUI`, `*.officeSL`), `XLSTART`, Word `STARTUP`, Quick Parts and add-ins; **jetbrains** copies the settings subfolders of each `%APPDATA%\JetBrains\<product><version>` (not plugins, licence or password database) and lists the products.
- **Listed names in the checklist (M9):** a `list_only` path may have `include`/`exclude` globs, matched against the listed names. In `notes`, the new optional `backup_step` and `post_restore`, `{each:<leaf>}` stands for the names listed by the `list_only` path whose template ends in `<leaf>` (`%KF:X%` counts as `X`; case-insensitive; "none found" if empty; at most 30, then "and N more"); validation rejects a placeholder that names no such path. `notes` are filled in at backup time (the manifest holds the filled text). `backup_step` becomes a step on the backup's checklist ("<pack> (<user>): before you leave the old PC"), for each user the pack is kept for: things to do on the old PC while it still runs.
- Last v1 packs (M9): **minecraft** copies Java Edition worlds, screenshots, resource/shader packs, mods, `config`, options, `servers.dat` and the launcher profiles (old sign-in tokens blanked), never versions/libraries/assets or the launcher's account files; Bedrock worlds and packs from `%APPDATA%\Minecraft Bedrock\Users` (GDK build) and the older Store app's `com.mojang` (Marketplace cache left out). **my-games** lists the games in `Documents\My Games` and `Saved Games` (both copied as user folders) and says saves elsewhere need a custom folder. **quickbooks** (users with `%LOCALAPPDATA%\Intuit\QuickBooks`) lists the company files (`*.qbw`, `*.qbb`, `*.qbm`) in Public Documents and adds a backup step: make a QuickBooks backup (or use the Migrator Tool) before leaving the old PC; company files are not copied by the pack (Full mode copies `Users\Public`).
- Media/gaming packs (M9): **obs** copies scenes, profiles and settings with stream keys, account tokens and the WebSocket password stripped (whole files only in the bundle); **adobe-presets** copies Camera Raw/Lightroom Classic presets and profiles, Lightroom templates and Photoshop presets and settings; **steam** (for users with HKCU `Software\Valve\Steam`) copies `userdata`, lists the installed games (named in the checklist), and leaves login data and games behind; re-linking a games library is a manual step.

---

## 7. Offline environments

### 7.1 BitLocker unlock (inside the tool)
Detect a BitLocker volume and prompt for the 48-digit recovery key (shown as typed, in groups of 6 like Windows' own recovery screen, with a digit count and the first mistyped group; never logged; the CLI rewrites its prompt line after Enter so the key isn't left in the console). The key can be pasted (Ctrl+V / Shift+Insert, from the Windows clipboard) in the CLI, text screens and GUI; a whole key is picked out of pasted text, e.g. a line copied from aka.ms/myrecoverykey with its key ID and date.
- WinPE: `manage-bde -unlock <X:> -RecoveryPassword <key>` (the image needs `WinPE-SecureStartup`). Also offered from Windows for another disk (GUI and text screens).
- Linux: `cryptsetup open --type bitlk --readonly` (≥2.3), key on stdin; fallback `dislocker -r` (key in its arguments: dislocker has no other non-interactive input).
- The key is checked first (48 digits, each group a multiple of 11 below 720 896), held as a secret, and scrubbed from any tool output shown.
- **Or the drive's BitLocker password** (data drives, and OS drives on PCs without a TPM): CLI `open --password` / `--password-file`, Tab in the text screens, a "Password" choice in the GUI; shown as dots with a character count. Windows/WinPE: WMI `Win32_EncryptableVolume.UnlockWithPassphrase` through PowerShell, the password on stdin (base64 of UTF-8), never on a command line (WinPE also needs WinPE-WMI and WinPE-PowerShell). Linux: the same cryptsetup call (it tries every key slot with what it's given); dislocker `-u`.
- A **TPM PIN can't** be used: the TPM releases the key only to the original PC's own Windows boot (its measurements differ under WinPE, Linux or another PC), so such a drive opens offline only with its recovery key. The screens say so.

### 7.2 Mounting (Linux)
**Read-only always** (sources; backup targets are the one exception, §8): `mount -t ntfs3 -o ro`, falling back to `ntfs-3g -o ro`. Fast Startup/hibernated and dirty volumes are mounted read-only and the user is told why. Mounts go under `/run/rebootly/<device>` and are undone at exit (`rebootly-cli open` leaves them for later commands; `rebootly-cli close` undoes them). Hibernation (Fast Startup) is detected from the `HIBR` header of `hiberfil.sys` on every offline volume (also WinPE) and shown in pre-flight and the report.

### 7.3 WinPE limits
- The X: RAM disk has tiny scratch space: write nothing there except state to the target.
- No Wi-Fi; a NAS needs Ethernet plus the networking component; `net use` with credentials.
- No WOW64: ship x64 and arm64 builds.

---

## 8. Rescue USB (Ventoy-based) — agreed optional step

```
USB (Ventoy)
├─ [Ventoy EFI partition]
└─ VENTOY (exFAT or NTFS data partition; not FAT32)
   ├─ .rebootly-root                  # marker: Rebootly finds its stick by it
   ├─ iso/
   │  ├─ rebootly-rescue-x86_64.iso   # rescue/alpine/build.sh: cryptsetup, ntfs3/ntfs-3g, exFAT; auto-starts the text screens
   │  ├─ rebootly-rescue-x86_64.sources.txt  # GPL source notice
   │  └─ rebootly-winpe-amd64.iso     # user-built: rescue/winpe/build.ps1 (ADK + WinPE add-on)
   ├─ bin/win-x64/{Rebootly.exe,rebootly-cli.exe}
   ├─ bin/win-arm64/…                 # optional
   ├─ bin/linux-x64/rebootly-cli      # static (musl), any x86-64 Linux
   ├─ packs/
   ├─ Rebootly/<MACHINE>_<YYYY-MM-DD_HHMM>/   # backups (the drive target folder, §11)
   ├─ ventoy/ventoy.json              # menu names, VTOY_DEFAULT_SEARCH_ROOT=/iso, VTOY_LINUX_REMOUNT=1
   ├─ start.cmd / start.sh            # pick the right binary
   └─ README.txt, GUIDE.txt
```

- Filled by `rescue/make-stick.ps1 -Drive E:` (or `-OutDir` → `dist/Rebootly-<v>-stick.zip` from `scripts/package-release.ps1`). The user installs Ventoy themselves (`Ventoy2Disk`).
- **WinPE is not redistributable** (license): ship the build script, not the image. `build.ps1`: `copype`, then `WinPE-WMI`, `WinPE-NetFX`, `WinPE-Scripting`, `WinPE-PowerShell`, `WinPE-StorageWMI`, `WinPE-EnhancedStorage`, `WinPE-SecureStartup` (+ language packs), 512 MB scratch space, `MakeWinPEMedia /ISO`. The image is generic: `startnet.cmd` runs `wpeinit` and `rebootly.cmd`, which finds the stick and calls its `start.cmd` (text screens; WinPE has no GPU for the window).
- The Alpine image is built from an open-source script and is fine to distribute (it includes GPL tools: the build writes the source notice). `build.sh` runs on Alpine (or in an `alpine` container: `build-in-docker.sh`/`.ps1`), builds `rebootly-cli` statically for musl, and makes the ISO with aports' `mkimage.sh` and a profile (`mkimg.rebootly.sh`) and overlay (`genapkovl-rebootly.sh`): `tty1` runs `rebootly-start` instead of a login (mounts the stick, runs its `start.sh`, then `rebootly-cli close` and a menu: again, keyboard layout, network, shell, restart, power off); no network at boot. No dislocker (Alpine has it only in testing): cryptsetup only.
- **Writing on Linux** happens only to backup targets: `rebootly-cli target --stick` (the volume with `.rebootly-root`) or `target <device>` / the text screens' "Use /dev/sdX…" entries, mounted read-write under `/media/<label>`. Every candidate is probed read-only first; a disk with Windows on it, or a locked one, is refused. `rebootly-cli close` syncs and unmounts the targets too. Ventoy holds its data partition after a Linux boot; `VTOY_LINUX_REMOUNT` lifts that (experimental in Ventoy), otherwise a second USB drive is the target.
- WinPE never offers its X: RAM disk as a target (§7.3).
- Secure Boot: Ventoy needs its MOK enrolled once, or the user disables Secure Boot. Documented in the stick's README.txt.
- In WinPE/Alpine the tool finds its own stick by looking for the marker file (`.rebootly-root`).

---

## 9. Security

- **Sensitive bundle** (Wi-Fi keys, certs, SSH keys, cloud creds, any password CSV) is always encrypted. Decided (M4): **age v1 with a scrypt passphrase** (`age` crate), holding a tar archive whose last entry is `rebootly-sensitive.json` (path, source, size, SHA-256 per file). Standard tools open it without Rebootly: `age -d sensitive.age | tar -x`. Passphrase from a prompt (`rebootly_tui::prompt::passphrase`: a dot per character and the count; typed or pasted, Ctrl+V / Shift+Insert read the clipboard; Esc cancels; asked twice for a backup; piped input is read one line per question), `--passphrase-file` or `$REBOOTLY_PASSPHRASE`; at least 8 characters; never stored or logged. The bundle is rebuilt on every completed run (also on resume). Files over 64 MiB are refused. `rebootly-cli sensitive list|extract` opens it; extraction refuses unsafe paths and verifies every hash.
- Optional **whole-backup encryption** (per-file, so it still resumes and dedups). Plain folders remain the default.
- Recovery keys and passphrases are never written to logs or the manifest.
- No network calls except to targets the user chose. No telemetry.
- Code signing for releases (Authenticode). This matters for SmartScreen/AV, because hive reading plus browser profile access looks suspicious when unsigned.

---

## 10. Manifest (`manifest.json`, schema-versioned)

Tool version, schema version, job ID, timestamps, `complete` flag; source machine name, OS edition/build, environment (live/winpe/linux); per user: SID, name, account type, profile path, KNOWNFOLDERID→path map; pack IDs and versions used; per file: relative path, size, mtime, attributes, SHA-256, status (ok/skipped/failed + reason); recorded reparse points; excluded categories; encryption info (no secrets); the Manual-steps list. Also (M2): per-user `hive_dirty`, pre-flight path `issues`, and `warnings`. Also (M4): `sensitive` (bundle file, format, and per file only path, source and size; hashes stay inside the encrypted bundle); per file `snapshot` (read from VSS) and `streams` (name, size, stored path, SHA-256, status). Folders not copied stay in the KNOWNFOLDERID map with `included: false` and a note. `job.state` is JSON lines (header + one `FileEntry` per file, same shape as the manifest's); a file is skipped on resume when its size and mtime are unchanged and the copy on the target has the right size. Raw hives go to `system/hives/<user>/`.

### Backup folder layout
```
<MACHINE>_<DATE>/
├─ manifest.json
├─ report.html  report.txt        # human-readable: what was done, failures, manual steps
├─ job.state                       # resume info
├─ users/<username>/
│  ├─ folders/<KNOWNFOLDER-name>/...
│  ├─ packs/<pack-id>/...
│  └─ registry/*.reg
├─ system/ (wifi/, printers/, tasks/, apps.json, hosts, hives/ raw)
├─ custom/<label>/...
├─ streams/<dest>/<stream-name>    # alternate data streams (live)
└─ sensitive.age                    # encrypted bundle
```
In `report.txt`/`report.html`, 5 or more failed files with the same report row (`users/<u>/<kind>/<name>`, `system/<item>`) and the same reason are one line (their deepest common source folder + count); "Access is denied" (os error 5) and "in use" (32/33) failures add a plain-language hint.

---

## 11. UX / ease of use

- Three modes on the start screen: **Quick** (Tier 1 + detected packs + Tier 4 system items, which are small and carry the app list), **Full** (everything detected: also `Users\Public`, every drive-root folder, and keys/credentials into the encrypted bundle), **Custom** (tree with checkboxes and sizes). CLI: default = Quick, `--full`, or the individual flags.
- An automatic **"Rescue" flow** when started in WinPE/Linux: find Windows → unlock BitLocker → pick user(s) → choose target → go.
- On restore, backups on the stick/NAS are found automatically and the newest is offered.
- Plain-language pre-flight summary: "47.2 GB, ~38 min to this USB, 12 apps to reinstall, 3 things you'll need to do by hand."
- Progress with ETA and current file; pause, resume and safe cancel; completion sound or notification.
- Saved **job profiles** on the stick (for repeat backups and for IT techs doing many PCs), as `<target>\rebootly-profiles\<name>.json` (on a drive the target is `<drive>\Rebootly\`). A CLI mirror of every TUI action (`rebootly-cli backup --profile FILE`, `--save-profile NAME`).
- Printable **Manual steps checklist** (`report.html` beside `report.txt`, self-contained), also shown at the end of backup and restore with tick-boxes.
- In live mode before backup: "Deactivate licenses on this PC now" checklist, and a "save your BitLocker recovery key" reminder.
- **Verify** command: re-hash a backup against its manifest. **Compare** command: after restore, check the new PC against the manifest. Implemented (M9): `rebootly-cli verify BACKUP [--sensitive|--passphrase-file F] [--all] [--json]` re-hashes every copied file and stored stream (missing / wrong size / SHA-256 / unreadable), lists files the manifest doesn't name (e.g. a leftover `.rbpart`; not a failure), counts files the backup itself failed (not checked), and with the passphrase decrypts `sensitive.age` (extraction checks every hash; manifest names missing inside = failed); an incomplete backup is a problem. `rebootly-cli compare BACKUP [--user NAME] [--map-drive D=E] [--no-packs] [--no-system] [--sensitive] [--simulate-root DIR]` builds the restore's own file plan for the user running it and checks each destination: same; same after the profile-path rewrite; kept beside as `(migrated)`; merged (hosts, phonebooks: present, not compared); this PC's own shortcut/`desktop.ini` kept; left out by design (SID-named, binary with old paths); changed on this PC after the backup (mtime > backup + 2 s); different; missing; blocked by a folder/link; unreadable. Only different/missing/blocked/unreadable fail. Settings (registry, Wi-Fi, power plan, certificates, apps) aren't compared. Both exit 0 = good, 2 = problems found, 1 = error; text lists stop at 50 per kind (`--all`). Core: `rebootly-core::check`. Not in the GUI/text screens yet.
- Logs: rotating, on the target, with secrets redacted. `--debug` flag.

---

## 12. Later (not v1)
Incremental/scheduled backups with a content-addressed store and dedup; zstd compression; SFTP/WebDAV targets; Storage Spaces/dynamic disks offline; restore from an offline environment (files only); a community pack repository.

---

## 13. Testing
- Unit: hive parsing against fixture hives (clean, dirty, Win10, Win11, Entra ID profile); env-var expansion; exclusion and priority logic; pack schema validation; path rewriting.
- Integration: fake profile trees in `tests/fixtures` with junction loops (on Windows CI), placeholders, long paths, 4 GB+ files (sparse), locked files.
- VM matrix (manual/scripted): Win10 → Win11 live; Win11 → Win11 different username; WinPE backup of a BitLocker disk; Alpine backup of a BitLocker disk; NAS dropout resume; FAT32 target.
- CI: GitHub Actions — `windows-latest` (msvc x64 + arm64 cross), `ubuntu-latest` (musl). `cargo clippy -D warnings`, `cargo test`, `cargo deny`.

---

## 14. Milestones
1. **M1 Core resolver:** workspace scaffold, hive parser integration, `ProfileList` + `User Shell Folders` resolution with env expansion, dirty-hive detection. CLI `rebootly-cli inspect --windows-root <path>` prints users and real folder paths. Tests on fixture hives.
2. **M2 Copy engine:** read-only, priority-ordered, no reparse traversal, retry and continue, hashing, manifest, resume, exclusions, pre-flight checks. CLI `backup` (Tier 1 only).
3. **M3 Packs:** schema, loader, detection, the first 8 packs, custom folders, path-rewrite metadata.
4. **M4 Live Windows:** SeBackupPrivilege, VSS, KnownFolder cross-check, OneDrive placeholders, winget/netsh/printers/tasks capture, sleep prevention, sensitive bundle encryption.
5. **M5 Restore:** phases, remapping, conflict policy, dry run, undo, registry import allowlist, verify/compare.
6. **M6 TUI:** Quick/Full/Custom, Rescue flow, progress, reports.
7. **M7 Offline:** WinPE detection, BitLocker unlock (WinPE + Linux), Linux ro mount, volume discovery.
8. **M8 Rescue USB:** Ventoy layout, Alpine build script, WinPE build script, start scripts, docs.
9. **M9 Hardening:** the rest of the pack set, VM test matrix, signing, release pipeline.
