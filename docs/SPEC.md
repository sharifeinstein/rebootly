# SPEC — Portable PC Migration & Backup Tool (name: **Rebootly**, binary `rebootly`)

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
- **UI:** `ratatui` TUI is the primary interface, because it works in a WinPE console, a Linux TTY and over SSH. An optional Slint GUI for live Windows comes later, on the same core.
- **Registry hives:** a pure-Rust offline hive parser, used in **all** modes. **Our own** read-only parser (`rebootly-core::hive`); `nt-hive` was rejected because it is GPL-2.0-or-later (decided M1, see PROGRESS.md). It is cross-platform and also covers users who aren't logged in. In live mode the Win32 API is a cross-check only.
- **Serialization:** `serde` with YAML for packs and JSON for the manifest.
- **Targets:** `x86_64-pc-windows-msvc` (+crt-static), `aarch64-pc-windows-msvc`, `x86_64-unknown-linux-musl`.
- **License:** Apache-2.0 (decided 2026-11-09). Track dependency licenses with `cargo-deny`.

### Workspace layout

```
rebootly/
├─ crates/
│  ├─ rebootly-core/    # engine, Source trait, packs, manifest, hive parser, resolver, copy/verify, report
│  ├─ rebootly-win/     # VSS, KnownFolder API, live registry, SeBackupPrivilege, manage-bde, winget, netsh
│  ├─ rebootly-linux/   # volume discovery, BitLocker unlock (cryptsetup/dislocker), ro mount
│  ├─ rebootly-cli/     # thin CLI (scripting, tests, IT techs)
│  └─ rebootly-tui/     # primary interactive front end
│  (rebootly-gui/ later — Slint, live Windows only)
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

### 3.4 Profile and folder resolution
- Profiles come from `SOFTWARE\Microsoft\Windows NT\CurrentVersion\ProfileList`: SID → `ProfileImagePath`. Skip system SIDs (S-1-5-18/19/20). Label local, Microsoft and Entra ID/domain accounts.
- Folders: each user's `NTUSER.DAT` → `Software\Microsoft\Windows\CurrentVersion\Explorer\User Shell Folders`. **Expand `%USERPROFILE%` and other variables manually**, using that user's profile path. Fall back to `Shell Folders`, then to the defaults.
- **Dirty hives:** after a crash, `NTUSER.DAT` may have unapplied `.LOG1/.LOG2` transaction logs. Detect this and warn. Support log replay if the parser can; otherwise read the base hive and flag it in the report. Always copy the hive and its logs into the backup as raw files.
- In live mode, cross-check against `SHGetKnownFolderPath` for the current user.
- The manifest stores **KNOWNFOLDERID → resolved source path** for every folder.

### 3.5 Copy engine
- **Never follow reparse points.** Junctions such as `Application Data`, `My Documents` and `Local Settings` cause infinite loops. Symlinks and junctions are recorded in the manifest, not traversed.
- OneDrive Files On-Demand placeholders (`RECALL_ON_DATA_ACCESS`/`OFFLINE` attributes, cloud reparse tag): user policy is skip, hydrate (live only), or skip the whole folder. Implemented (M4): `--cloud-files skip` (default; excluded category + Manual step) and `--cloud-files hydrate` (live only; pre-flight warns how much will be downloaded onto the source disk). Skip-whole-folder is not implemented.
- Live mode: `SeBackupPrivilege` plus `FILE_FLAG_BACKUP_SEMANTICS`, to read other users' profiles past their ACLs (admin required). Locked files are read from a **VSS snapshot** when elevated; otherwise prompt "close app X". Implemented (M4): a sharing or lock violation switches that file to a shadow copy at once (no backoff); the shadow is made lazily, once per volume, through WMI `Win32_ShadowCopy.Create` (ClientAccessible) run by PowerShell, and deleted when the job ends. IDs are kept in `%TEMP%\rebootly-vss-<pid>.txt` so the next live run deletes shadows left by a killed run. If no shadow can be made, the file fails at once with "close the program and run --resume". The manifest marks such files `snapshot: true`.
- Long paths (`\\?\`), timestamps and attributes preserved. ACLs optional (off by default). Alternate data streams: drop `Zone.Identifier` by default (configurable); record others. Implemented (M4, live only): other streams are listed from a backup-semantics handle and stored as separate files `streams/<dest>/<name>` (targets like exFAT/NAS can't hold streams), recorded per file in the manifest; `--keep-zone-identifier` keeps that one too; streams over 64 MiB are listed, not stored. Offline mounts don't expose streams.
- **Priority order:** Desktop, Documents, Pictures → app packs/settings → Music, Videos → Downloads → custom folders. The user can reorder.
- **Failing-disk tolerance:** retry a read N times with backoff, then log and continue. Never abort the whole job over one file. If SMART data looks bad (when readable), suggest imaging with `ddrescue` first.
- **Resumable:** the job state file on the target lets the copy resume after a NAS dropout or cancel. Pause and resume are supported.
- Parallel, bounded I/O. A SHA-256 hash per file goes into the manifest (computed while streaming).
- **Pre-flight:** size per category, target free space, **FAT32 4 GB limit detection** (warn, suggest exFAT/NTFS), path-length and invalid-character checks for SMB/Linux targets, case-collision check for case-sensitive NAS. The target's filesystem type comes from /proc/self/mountinfo on Linux and `GetVolumeInformationW` on Windows.
- Live mode: prevent sleep during the job (`SetThreadExecutionState`). Warn if a laptop is on battery.

### 3.6 Exclusions (always)
`pagefile.sys`, `hiberfil.sys`, `swapfile.sys`, `$Recycle.Bin`, `System Volume Information`, `%TEMP%`, `*.ost`, `Thumbs.db`, `INetCache`, `CrashDumps`, `WER`, every browser cache dir (`Cache`, `Code Cache`, `GPUCache`, `Service Worker\CacheStorage`, `ShaderCache`), `Packages\*\AC`, `Packages\*\TempState`. `NTUSER.DAT*` and `UsrClass.dat*` are copied raw into `_system/hives/`, never restored over a live profile. Optional: `node_modules`, `.git/objects`. Keep `desktop.ini`.
**Never copy** browser `Login Data`, `Cookies`, `Local State` encryption keys, or Credential Manager vaults (they can't decrypt elsewhere, and copying them looks like infostealer behaviour).
Implemented list (M2): Chromium `Login Data*`, `Cookies`, `Local State`; Firefox `logins.json`, `key3/key4.db`, `cookies.sqlite`, `signons.sqlite`; `Microsoft\Credentials`, `Microsoft\Vault`, and the DPAPI `Microsoft\Protect` folder. Their SQLite sidecars (`-journal`, `-wal`, `-shm`) count as the same secret (M5). AppData-only rules (caches, secrets) match only under `AppData\`, so a user's own `Documents\Cache` is still copied.

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

Implemented (M4, part 1): `system/hosts`; scheduled tasks outside `Tasks\Microsoft` → `system/tasks/`; Wi-Fi profile XMLs (offline) → the sensitive bundle only (`system/wifi/`); `system/apps.json` (machine-wide Uninstall entries). Live only: `system/winget.json` (`winget export --include-versions`), `system/printers.json` (`Get-Printer`), and with `--sensitive` `netsh wlan export profile key=clear` into the bundle (`system/wifi-export/`; the clear-text XMLs exist only in a private temp folder during the job). Each command has a 180 s timeout; failures become warnings. `--no-system` leaves all of this out.
Implemented (M4, part 2): per-user `rasphone.pbk` via the `windows-shell` pack (v2), all-users one → `system/vpn/rasphone.pbk` (both modes; it holds no passwords). Live only: `system/power/active-scheme.pow` (`powercfg /export` of the active scheme), `system/default-apps.xml` (`dism /Export-DefaultAppAssociations`), `system/drivers.txt` (`pnputil /enum-drivers`, reference only). User certificates: with `--sensitive`, `Cert:\CurrentUser\My` of the user running the tool is exported with `Export-PfxCertificate` (instead of certutil, which exports one certificate per call and takes the password on its command line) into the bundle as `system/certs/<thumbprint>.pfx` (`.cer` when there's no key or it isn't exportable, plus a warning), protected by a random password stored next to them as `system/certs/PFX-PASSWORD.txt` inside the bundle. Other users' certificates need their own sign-in.

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

Implemented (M5): `rebootly restore <BACKUP>` and `rebootly undo [UNDO_DIR] [--list]`. One run restores one backup user (`--user NAME`; picked automatically when there is only one, or one with the same name) into the user running Rebootly, whose known-folder paths come from `SHGetKnownFolderPath` (so OneDrive folder backup on the new PC is followed). Needs an elevated prompt (except `--dry-run`). Mapping: `users/<u>/folders/<KF>` → the new path of that known folder; pack files → the pack template expanded for the new user (a destination outside the profile is allowed only when the template is a path of the pack installed in this Rebootly); `custom/<label>` → the old path with old profile → new profile, old `Users\Public` → new, then `--map-drive D=E`; never into `Windows` or `Program Files`. Raw hives, `drivers.txt` and `apps.json` are reference only. Conflicts: identical files (size + SHA-256) are always skipped; default policy is keep-both for folders and custom folders, overwrite for packs and system files (`--conflict`, `--conflict-for folders=skip,packs=ask`); `overwrite` was added to the four policies because a freshly installed app's default settings are newer than the old ones. A `desktop.ini` that already exists is kept, and so is an existing shortcut (`.lnk`/`.url`: never replaced or duplicated, the new PC's one points to the right places). Files named after the old account's SID are skipped, and so are binary pack files with the old profile path inside when the profile path changes (Quick Access pins, shortcuts): they can't be rewritten. Hosts and `rasphone.pbk` (both) are merged: missing entries/connections are appended. Files are written as `.rbpart`, checked against the backup's SHA-256, renamed into place, mtime kept, streams written back, hidden/system attributes set; read back and re-hashed in the verify phase. Path rewriting (`rewrite: true`): `C:\Users\old` in its `\`, `\\` and `/` spellings, whole path components only, in UTF-8 (with or without BOM) and UTF-16LE text up to 16 MiB. Registry: each pack's `.reg` must pass the allowlist of that pack *as installed* (only `HKEY_CURRENT_USER` keys under its `registry:` entries, no key or value deletions); profile paths in string values are rewritten; `HKCU\Environment` and `HKCU\Network` are flagged for review in the plan. Before importing, each top-level key is exported to the undo folder (absent keys are recorded, and deleted on undo). Phase 3 is a prompt after `winget import` (`--apps install|skip|only`). Packs whose `requires_closed` process is running are skipped with a "close it and run again" note. System: Wi-Fi from `system/wifi-export/` (bundle) via `netsh wlan add profile`; offline Wi-Fi XMLs only become a "re-join" checklist; power plan: if a scheme with the old active plan's GUID exists (built-in plans share GUIDs), it is made active, otherwise the exported plan is imported and made active; certificates imported into `Cert:\CurrentUser\My` (PFX password read from the bundle file by PowerShell, never on a command line), only when restoring the user who ran the backup (`system/certs/OWNER.txt` in the bundle holds their SID; without it, nothing is imported and a manual step explains why); scheduled tasks only with `--tasks` (apps usually re-create their own); printers, default apps and drivers are manual steps. Existing Wi-Fi profiles, certificates and tasks are left alone. Explorer is restarted only after a registry import or a `windows-shell` pack change. The sensitive bundle is opened with `--sensitive` into a private temp folder that is deleted afterwards. Undo: `%USERPROFILE%\.rebootly-undo\<job>_<time>\` holds `journal.jsonl` (written and fsynced *before* each change), the replaced files (`files/`), exported keys (`reg/`) and `report.txt`; undo walks the journal backwards, leaves files changed since the restore alone (unless `--force`), removes created folders if empty, deletes imported Wi-Fi profiles, certificates, tasks and the power plan, and marks the run `undone.txt`. `--simulate-root DIR` restores files under `DIR/C/…` (system steps only listed) for previews and tests off Windows. Not yet: `min_app_version`, offline file-only restore, read-only attributes, ACLs.

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
- Schema details (M3): optional `version` (pack content version, default 1); per-path `exclude` globs and `sensitive: true`; detect conditions are one-key maps: `path`, `uninstall_name` (glob on DisplayName, machine and user Uninstall keys), `registry` (HKCU key exists), `always`. Path templates may use `%KF:<Folder>%` for a resolved known folder (e.g. `%KF:Documents%\\PowerShell`, which follows OneDrive redirection). `registry` keys must be HKCU and are exported recursively to `users/<u>/registry/<pack-id>.reg` (UTF-16LE, regedit 5.00). Copied paths go to `users/<u>/packs/<id>/<leaf>/`. Built-in packs are compiled into the binary; YAML files in `packs/` with the same `id` replace them. A pack detected only by a machine-wide install, with nothing of that user's on disk, is dropped for that user. `sensitive` paths go only into `sensitive.age` (status `encrypted`); without a passphrase they are not copied at all (status `sensitive_not_copied`, plus a Manual step). Either way a parent folder never walks into them, so they are never copied in plain.

---

## 7. Offline environments

### 7.1 BitLocker unlock (inside the tool)
Detect a BitLocker volume and prompt for the 48-digit recovery key (input masked, never logged).
- WinPE: `manage-bde -unlock <X:> -RecoveryPassword <key>` (the image needs `WinPE-SecureStartup`).
- Linux: `cryptsetup bitlkOpen` (≥2.3); fallback `dislocker`.

### 7.2 Mounting (Linux)
**Read-only always:** `mount -t ntfs3 -o ro`, falling back to `ntfs-3g -o ro`. Fast Startup/hibernated and dirty volumes are mounted read-only and the user is told why.

### 7.3 WinPE limits
- The X: RAM disk has tiny scratch space: write nothing there except state to the target.
- No Wi-Fi; a NAS needs Ethernet plus the networking component; `net use` with credentials.
- No WOW64: ship x64 and arm64 builds.

---

## 8. Rescue USB (Ventoy-based) — agreed optional step

```
USB (Ventoy)
├─ [Ventoy EFI partition]
└─ VENTOY (exFAT data partition)
   ├─ iso/
   │  ├─ rebootly-rescue-alpine.iso   # built by rescue/alpine/build.sh: ntfs3, cryptsetup, dislocker, auto-starts TUI
   │  └─ winpe-rebootly.iso           # user-built via rescue/winpe/build.cmd (ADK + WinPE add-on)
   ├─ bin/win-x64/rebootly.exe
   ├─ bin/win-arm64/rebootly.exe
   ├─ bin/linux-x64/rebootly
   ├─ packs/
   ├─ backups/<MACHINE>_<YYYY-MM-DD_HHMM>/
   ├─ start.cmd / start.sh           # pick the right binary
   └─ README.txt
```

- **WinPE is not redistributable** (license): ship the build script (`copype`, add `WinPE-SecureStartup`, `WinPE-WMI`, `WinPE-Scripting`, networking; `startnet.cmd` launches the TUI from the stick), not the image.
- The Alpine image is built from an open-source script and is fine to distribute (it includes GPL tools, so attach source/notices).
- Secure Boot: Ventoy needs its MOK enrolled once, or the user disables Secure Boot. Document both.
- In WinPE/Alpine the tool finds its own stick by looking for a marker file (`.rebootly-root`).

---

## 9. Security

- **Sensitive bundle** (Wi-Fi keys, certs, SSH keys, cloud creds, any password CSV) is always encrypted. Decided (M4): **age v1 with a scrypt passphrase** (`age` crate), holding a tar archive whose last entry is `rebootly-sensitive.json` (path, source, size, SHA-256 per file). Standard tools open it without Rebootly: `age -d sensitive.age | tar -x`. Passphrase from a hidden prompt (asked twice), `--passphrase-file` or `$REBOOTLY_PASSPHRASE`; at least 8 characters; never stored or logged. The bundle is rebuilt on every completed run (also on resume). Files over 64 MiB are refused. `rebootly sensitive list|extract` opens it; extraction refuses unsafe paths and verifies every hash.
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

---

## 11. UX / ease of use

- Three modes on the start screen: **Quick** (Tier 1 + detected packs + Tier 4 system items, which are small and carry the app list), **Full** (everything detected: also `Users\Public`, every drive-root folder, and keys/credentials into the encrypted bundle), **Custom** (tree with checkboxes and sizes). CLI: default = Quick, `--full`, or the individual flags.
- An automatic **"Rescue" flow** when started in WinPE/Linux: find Windows → unlock BitLocker → pick user(s) → choose target → go.
- On restore, backups on the stick/NAS are found automatically and the newest is offered.
- Plain-language pre-flight summary: "47.2 GB, ~38 min to this USB, 12 apps to reinstall, 3 things you'll need to do by hand."
- Progress with ETA and current file; pause, resume and safe cancel; completion sound or notification.
- Saved **job profiles** on the stick (for repeat backups and for IT techs doing many PCs), as `<target>\rebootly-profiles\<name>.json` (on a drive the target is `<drive>\Rebootly\`). A CLI mirror of every TUI action (`rebootly backup --profile FILE`, `--save-profile NAME`).
- Printable **Manual steps checklist** (`report.html` beside `report.txt`, self-contained), also shown at the end of backup and restore with tick-boxes.
- In live mode before backup: "Deactivate licenses on this PC now" checklist, and a "save your BitLocker recovery key" reminder.
- **Verify** command: re-hash a backup against its manifest. **Compare** command: after restore, check the new PC against the manifest.
- Logs: rotating, on the target, with secrets redacted. `--debug` flag.

---

## 12. Later (not v1)
Incremental/scheduled backups with a content-addressed store and dedup; zstd compression; SFTP/WebDAV targets; Slint GUI; Storage Spaces/dynamic disks offline; restore from an offline environment (files only); a community pack repository.

---

## 13. Testing
- Unit: hive parsing against fixture hives (clean, dirty, Win10, Win11, Entra ID profile); env-var expansion; exclusion and priority logic; pack schema validation; path rewriting.
- Integration: fake profile trees in `tests/fixtures` with junction loops (on Windows CI), placeholders, long paths, 4 GB+ files (sparse), locked files.
- VM matrix (manual/scripted): Win10 → Win11 live; Win11 → Win11 different username; WinPE backup of a BitLocker disk; Alpine backup of a BitLocker disk; NAS dropout resume; FAT32 target.
- CI: GitHub Actions — `windows-latest` (msvc x64 + arm64 cross), `ubuntu-latest` (musl). `cargo clippy -D warnings`, `cargo test`, `cargo deny`.

---

## 14. Milestones
1. **M1 Core resolver:** workspace scaffold, hive parser integration, `ProfileList` + `User Shell Folders` resolution with env expansion, dirty-hive detection. CLI `rebootly inspect --windows-root <path>` prints users and real folder paths. Tests on fixture hives.
2. **M2 Copy engine:** read-only, priority-ordered, no reparse traversal, retry and continue, hashing, manifest, resume, exclusions, pre-flight checks. CLI `backup` (Tier 1 only).
3. **M3 Packs:** schema, loader, detection, the first 8 packs, custom folders, path-rewrite metadata.
4. **M4 Live Windows:** SeBackupPrivilege, VSS, KnownFolder cross-check, OneDrive placeholders, winget/netsh/printers/tasks capture, sleep prevention, sensitive bundle encryption.
5. **M5 Restore:** phases, remapping, conflict policy, dry run, undo, registry import allowlist, verify/compare.
6. **M6 TUI:** Quick/Full/Custom, Rescue flow, progress, reports.
7. **M7 Offline:** WinPE detection, BitLocker unlock (WinPE + Linux), Linux ro mount, volume discovery.
8. **M8 Rescue USB:** Ventoy layout, Alpine build script, WinPE build script, start scripts, docs.
9. **M9 Hardening:** the rest of the pack set, VM test matrix, signing, release pipeline.
