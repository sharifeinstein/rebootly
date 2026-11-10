# PROGRESS

## How we work (read this first in every session)
- One milestone at a time. At the start of a milestone, read only the SPEC sections it needs (e.g. M2 → §3.5, §3.6, §10). Don't re-read the whole spec.
- Keep tool output small: `cargo build -q`, `cargo test -q 2>&1 | tail -40`, `cargo clippy -q`. Grep or read line ranges; never dump whole files or long logs.
- Small modules (~300 lines max per file).
- Tests alongside code. A milestone is done only when `cargo test` and `cargo clippy --all-targets -- -D warnings` pass and the milestone's CLI command works on fixtures.
- After every milestone: update this file, commit, give a ≤5-line summary (what works, what's next, decisions needed).
- A changed design decision goes in the Decisions log below **and** in docs/SPEC.md, so the two never drift apart.
- Say when the chat is getting long (about two milestones in) so a fresh chat can resume from this file.
- Windows-only code (VSS, KnownFolder API, privileges) behind `cfg(windows)`. Core must compile and test on Linux.
- No secrets in logs, ever. Never decrypt DPAPI data or read browser password/cookie stores.

## Repo / environment
- Canonical tree: `D:\Projects\rebootly` on the user's PC (repo root, with `.git`). GitHub: `sharifeinstein/rebootly` once the user creates it (the Claude session can't create repos).
- Builds run in the Claude cloud workspace (Rust 1.97, crates.io reachable); the tree is mirrored to D: after each milestone. The VM behind the linked folder has git but no Rust.
- Sync to D: (until GitHub is set up). Small updates: `git bundle create <out>.bundle <device-head>..main` in the cloud, commit it to `D:\Projects\rebootly\.rb-update.bundle`, then on the device `git pull --ff-only .rb-update.bundle main && rm .rb-update.bundle` (no loose-ref or leftover-file problems). To bring device commits to the cloud, the reverse. Full mirror: `git gc -q && tar --exclude=./target --exclude=./.git/objects/info/commit-graph --mode=u+w -cf /mnt/user-data/outputs/rebootly-sync-<sha>.tar .`, device_commit_files it to `D:\Projects\rebootly\.rebootly-sync.tar` (use a new staged file name each time, or a stale copy is sent), then on the device `tar --overwrite -xf .rebootly-sync.tar`. The bundle can't delete: after extracting, run `git status` on the device. Any untracked leftovers from files that were deleted or moved in the repo are moved (not deleted) to `D:\Projects\_rebootly_to_delete\`, which the user empties. `git gc` packs refs, and a stale loose `.git/refs/heads/main` on the device then still wins: if `git log -1` on the device shows the old commit, run `git update-ref refs/heads/main <new-sha> <old-sha>`. Always keep `--mode=u+w`: a file that arrives read-only on Windows can't be overwritten from the VM (the user would have to run `attrib -r /s`).
- **Windows checks — split of work (2026-11-09):** the user builds, tests and runs on the real Windows PC from PowerShell (`cargo build --release`, `cargo test`, `cargo clippy --all-targets -- -D warnings`, then the live commands) and pastes the output back. Claude doesn't spend effort on Windows tooling in the cloud beyond the cheap type-check.
- Cloud Windows type-check (optional, cheap): `scripts/check-windows.sh check|clippy` builds std from source for `x86_64-pc-windows-msvc`. Setup per new session: `git clone --depth 1 --filter=blob:none --sparse https://github.com/rust-lang/rust.git`, fetch the commit from `rustc -vV` (commit-hash), `git sparse-checkout set library`, check out `library/backtrace` (submodule, from rust-lang/backtrace-rs at the pinned sha), copy `library/` to `$(rustc --print sysroot)/lib/rustlib/src/rust/library`. `scripts/test-windows-wine.sh` (MinGW + wine64 via apt) also works for running the tests under Wine; use only if the user's PC isn't available.
- Sync needs file deletion in the D: folder (git removes its own lock files): call device_request_delete_permission for `D:\Projects\rebootly` once per session before any git command there. Exclude the bundle itself when re-bundling on the device (`--exclude=./.rebootly-sync.tar`).
- To resume in a new chat: copy `D:\Projects\rebootly` into the workspace (stage `.rebootly-sync.tar` after re-bundling on the device, or the files) (or clone from GitHub once it exists), then `cargo test`.

## Status
Current milestone: M5 — Restore (implemented 2026-11-10; waiting for the Windows VM test before it is ticked)
Last session: 2026-11-10 — M5 written and tested on Linux: `rebootly restore` (plan → apps → files → packs → registry → system → Explorer → verify → manual steps), conflict policies, dry run, undo journal + `rebootly undo`, `--simulate-root` for previews/tests off Windows. 119 tests pass; clippy clean on Linux and (cloud type-check) for `x86_64-pc-windows-msvc`. The Windows side (`rebootly-win::live::restore`: `live_target`, `WinRestoreHost`) has only been type-checked, never run.

## Next step
1. Test restore on the user's Windows 10 VM (VM `DESKTOP-D3P1F40`, user `Picard`, backup at `E:\rb\DESKTOP-D3P1F40_20261110-131615`, passphrase file `C:\rebootly\pp.txt`). Build on the PC (`cargo build --release`, `cargo test`, `cargo clippy --all-targets -- -D warnings`), copy `target\release\rebootly.exe` to the VM, elevated PowerShell:
   - `rebootly restore E:\rb\DESKTOP-D3P1F40_20261110-131615 --dry-run --sensitive --passphrase-file C:\rebootly\pp.txt` (paste the plan and the conflict decisions)
   - best on a second, fresh local user that is an administrator (e.g. `Riker`; sign in as Riker so HKCU and the profile are his): `rebootly restore <backup> --user Picard --sensitive --passphrase-file C:\rebootly\pp.txt` → check Desktop/Documents, VS Code/Git settings with rewritten paths, wallpaper/mouse (registry), the test certificate in `certmgr.msc`, power plan, hosts, `report.txt` in `%USERPROFILE%\.rebootly-undo\…`
   - then `rebootly undo --list` and `rebootly undo` → everything back
   - into the same user (`Picard`): expect "already there" for everything (identical files)
2. Things only Windows can tell: `reg export`/`reg import` of the allowlisted keys, `Test-Path Registry::…`, Explorer restart (AutoRestartShell, not elevated), `powercfg /import` GUID parsing on a non-English Windows, `Import-PfxCertificate` with the bundle password file, stream + hidden/system attribute writes, `winget import` (needs App Installer).
3. Then M6 (TUI). Open from M4: the hidden passphrase prompt got fewer than 8 characters twice on the VM (probably paste through the VM console); `--cloud-files hydrate`, Wi-Fi export and winget export are still untested for real.

## Milestones
- [x] M1 Core resolver
- [x] M2 Copy engine
- [x] M3 Packs
- [x] M4 Live Windows
- [ ] M5 Restore (code done; Windows VM test pending)
- [ ] M6 TUI
- [ ] M7 Offline (WinPE/Linux, BitLocker)
- [ ] M8 Rescue USB (Ventoy, Alpine, WinPE scripts)
- [ ] M9 Hardening & release

## Decisions log
- 2026-11-09: Own read-only regf parser instead of `nt-hive` — nt-hive is GPL-2.0-or-later, which would force the whole tool to GPL; SPEC allowed "or our own". SPEC §2 updated.
- 2026-11-09: License is Apache-2.0 (owner decision). `LICENSE` added; Cargo.toml and SPEC §2 updated. Dependencies must be Apache-2.0-compatible (enforce with cargo-deny).
- 2026-11-09: Fixtures are synthetic hives from a test-only writer (`hive::writer`, feature `fixtures`), committed under `tests/fixtures/` and checked for drift by a test. The writer only creates new files; the tool never writes hives to a source disk.
- 2026-11-09: `S-1-5-21` accounts are reported as `local/domain`. Telling local from AD-domain offline needs the machine SID from SECURITY/SAM; deferred rather than guessed. Microsoft accounts are detected via NTUSER `IdentityCRL\UserExtendedProperties`; Entra ID via SID prefix `S-1-12-1-`.
- 2026-11-09: `ProfileList\*.bak` entries are skipped and listed as skipped, alongside S-1-5-18/19/20.
- 2026-11-09: Offline, only the system drive letter (from `SystemRoot`) is mapped to the volume; paths on other letters are reported as "on drive X:, not mounted". Mapping other volumes via `MountedDevices` belongs to M7.
- 2026-11-09: WinPE detection in core shells out to `reg query …\MiniNT` (no Win32 deps in core); rebootly-win may replace it.

- 2026-11-09: Project named **Rebootly** (was working name `migrate`, taken on crates.io). Crates `rebootly-*`, binary `rebootly`, markers `.rebootly-root` / `.rebootly-undo`. SPEC updated. Project moved to `D:\Projects\rebootly` the same day.

- 2026-11-09: Backup usernames come from the ProfileList path leaf (registry casing, e.g. `Alice`), and get `_<RID>` appended if two collide. Raw hives (NTUSER.DAT, UsrClass.dat + logs) are copied first, to `system/hives/<user>/`.
- 2026-11-09: Copies stream bytes (not CopyFileEx), so *all* alternate data streams are dropped, including Zone.Identifier. Recording the other streams needs Win32 (M4). Each file is written as `<name>.rbpart`, fsynced, then renamed into place.
- 2026-11-09: `job.state` is JSON lines, flushed per file; the manifest is rebuilt from the plan plus state at the end. A resumed run skips files whose size and mtime are unchanged and whose copy on the target has the right size (no re-hash). If writing to the target fails (NAS dropout), the job stops cleanly so it can be resumed.
- 2026-11-09: The source-volume check is a hard blocker that `--force` can't override (SPEC §1.6). Other blockers (e.g. free space) can be forced.
- 2026-11-09: The CLI exits 0 when everything was copied, 2 when it finished with failed files, and 1 on error. Ctrl-C has no handler: the per-file `job.state` already makes `--resume` work. Pause/resume controls come with the TUI (M6).
- 2026-11-09: Never-copy list extended to Firefox credential/cookie stores and the DPAPI `Protect` folder (SPEC §3.6 updated).

- 2026-11-09: YAML via `serde_norway` (maintained serde_yaml fork, MIT/Apache, MSRV 1.71). Detect conditions are parsed as one-key maps, so packs keep the SPEC's `- path: ...` syntax (serde_norway alone would need `!path` tags).
- 2026-11-09: Packs are compiled into the binary (`include_str!`) and overridden by id from `packs/` beside the binary or `--packs-dir`. Bad pack files are warnings, not fatal; `rebootly pack validate` checks files before import.
- 2026-11-09: `%KF:<Folder>%` template variables point at resolved known folders, so packs follow redirection (OneDrive KFM).
- 2026-11-09: A pack root nested in a planned folder is copied once, by the pack, if the pack has no include/exclude filter. Filtered pack roots are also walked by the parent folder (copied twice rather than risk losing files).
- 2026-11-09: A pack detected only by a machine-wide Uninstall entry, with no files or registry for that user, is dropped for that user.
- 2026-11-09: Until the encrypted sensitive bundle exists (M4), `sensitive: true` paths are not copied; they're listed as `sensitive_not_copied` plus a manual step. Plain copies of keys are never made.
- 2026-11-09: Tier 3 (HKCU allowlist) and the Tier 2 shell folders are implemented as the built-in `windows-shell` pack (detect: always). Office keys come with an Office pack later.
- 2026-11-09: `rebootly backup` grows `--packs-dir`, `--pack ID`, `--no-packs`, `--public`, `--root-folder NAME`, `--custom LABEL=WINPATH`. Non-system drive-root folders are listed ("offered") in the output and manifest but never included automatically.

- 2026-11-09: Live mode requires elevation; without it `--live` fails with a clear message instead of a partial backup. Loaded hives are read from `RegSaveKeyExW` snapshots (the on-disk files are locked and lag behind); the snapshot is the raw hive kept in the backup, without logs. Snapshots live in `%TEMP%\rebootly-hives-*` and are deleted when the source is dropped.
- 2026-11-09: Sensitive bundle format: age v1 + scrypt passphrase, tar inside, index `rebootly-sensitive.json` last (SPEC §9). Opt-in with `--sensitive` (prompt / `--passphrase-file` / `$REBOOTLY_PASSPHRASE`, min 8 chars). Rebuilt on every completed run. Manifest holds names and sizes only.
- 2026-11-09: Sensitive pack roots are always in the walker's skip set (also without a passphrase), and the skip set compares paths case-insensitively. `resolve_ci` returns the on-disk casing. Both were found by running the tests as Windows binaries.
- 2026-11-09: Online-only cloud files (attributes OFFLINE / RECALL_ON_OPEN / RECALL_ON_DATA_ACCESS) are skipped by default (excluded category + a Manual step), so a live backup never triggers OneDrive downloads.
- 2026-11-09: dev-dotfiles pack v2: `.azure` and `.kube` exclude caches/logs/extensions.

- 2026-11-09: Tier 4 split: file items (hosts, non-Microsoft tasks, Wi-Fi XMLs, apps.json) are planned in core for both modes; live command captures run through `Source::system_captures` at plan time into a private temp folder (so pre-flight knows their sizes). Wi-Fi of any kind goes only into the encrypted bundle; `netsh … key=clear` runs only with `--sensitive`.

- 2026-11-09: Locked files (sharing/lock violation) switch to a VSS shadow at once; shadows are made lazily per volume through WMI `Win32_ShadowCopy.Create` via PowerShell (no `windows` crate COM bindings), deleted at the end, leftovers of killed runs deleted by the next live run. An `Unsupported` open error (no snapshot possible) fails the file at once with a "close it and --resume" hint. Manifest `snapshot: true`.
- 2026-11-09: PowerShell scripts are passed with `-EncodedCommand` (UTF-16LE base64), so quoting can't break them.
- 2026-11-09: Alternate data streams (live): stored as `streams/<dest>/<name>` and listed per file in the manifest; Zone.Identifier dropped unless `--keep-zone-identifier`; streams over 64 MiB listed only.
- 2026-11-09: `--cloud-files skip|hydrate`; hydrate is live only (CLI refuses it offline, core downgrades with a warning) and pre-flight warns about the download.
- 2026-11-09: User certificates via `Export-PfxCertificate` (not certutil), bundle only, random PFX password stored inside the bundle; running user only; non-exportable keys → `.cer` + warning. Power plan, default apps and driver list are plain live captures; VPN phonebooks are copied in both modes (per user: windows-shell pack v2; all users: `system/vpn/`).
- 2026-11-09: `backup::preflight` takes the target filesystem type from the caller (Windows: rebootly-win), else looks it up (Linux). `run.rs` per-file work moved to `backup::filecopy`.

- 2026-11-10: Windows builds link the C runtime statically (`.cargo/config.toml`, `+crt-static`), so `rebootly.exe` runs on a clean PC/VM/WinPE without the VC++ Redistributable.

- 2026-11-10: `Source::is_live()` says whether the source is the running Windows; `environment()` only says where the process runs (an offline backup of another disk made from Windows is still "live-windows"). Found by `cargo test` on Windows: hydrate's live-only check used `environment()`.

- 2026-11-10: Restore reads the manifest through a tolerant view (`restore::input::BackupManifest`); the record types got `Deserialize`. `time::parse_rfc3339` added.
- 2026-11-10: One restore run = one backup user → the user running Rebootly (HKCU and `SHGetKnownFolderPath` only work for that user). Elevation required except for `--dry-run`. The plan names the target user and profile and asks before starting (`--yes` to skip).
- 2026-11-10: Conflict policies gained `overwrite` (SPEC listed skip / overwrite-if-newer / keep-both / ask). Defaults: keep-both for folders and custom, overwrite for packs and system (a new app's default config is always newer than the old one). Identical files are always skipped, so a second run is cheap and safe.
- 2026-11-10: Registry import allowlist = the `registry:` entries of the pack *installed in this Rebootly* (built-in or `--packs-dir`), not anything the backup says; unknown packs' `.reg` files are refused. Pack destinations outside the profile need the same: the template must be a path of the installed pack.
- 2026-11-10: hosts and both `rasphone.pbk` files are merged (missing entries appended), not overwritten. Scheduled tasks are opt-in (`--tasks`). Existing Wi-Fi profiles, certificates and tasks are left alone. Offline Wi-Fi XMLs (keys DPAPI-protected) are never imported, only listed.
- 2026-11-10: Undo journal is write-ahead (`journal.jsonl`, fsync per entry) in `%USERPROFILE%\.rebootly-undo\<job>_<time>[-n]\`; undo keeps files changed after the restore unless `--force`, and marks the run `undone.txt`. Registry undo re-imports the exported keys and deletes keys that didn't exist; a key's existence is checked with `Test-Path Registry::…`, so a failing `reg export` never turns into a delete on undo.
- 2026-11-10: `--simulate-root DIR` (`Target::simulated`, `SimHost`): files are restored under `DIR/C/…` and system steps only listed. Used by the CLI tests and for previews off Windows; not the "offline file-only restore" of SPEC §5 (still later).
- 2026-11-10: A `desktop.ini` already at the destination is kept (Windows writes its own); restored ones get hidden+system. Only hidden/system attributes are restored (read-only would block a later restore or undo).
- 2026-11-10: VS Code extension folder names (`publisher.name-1.2.3`) lose the version in the `post_restore` checklist. `post_restore` commands are listed as manual steps, not run.

## Known issues / TODO
- Run for real on Windows 10 (2026-11-10): Get-Printer, powercfg, dism, pnputil, Export-PfxCertificate, WMI shadow copies, streams. Not yet: winget export and netsh Wi-Fi export (not available on the test VM), OneDrive hydrate. The core side is tested with the `FakeLive` double (`backup/tests/live_like.rs`).
- `--preflight` in live mode runs all the captures (seconds to a minute; with `--sensitive` also the Wi-Fi and certificate exports, into a temp folder that is deleted).
- VSS: a shadow copy is ClientAccessible and persists if the process is killed; the next live run deletes it (per-PID ledger in `%TEMP%`). Two live runs at once are not supported.
- Alternate data streams cost one extra open per file (listing); offline mounts don't expose streams (ntfs-3g `streams_interface=xattr` could, M7).
- Certificates are exported only for the user running the tool; `--cloud-files` has no "skip the whole OneDrive folder" choice yet.
- Offline, OneDrive placeholders can't be recognised (no attributes on Linux mounts); reading one may return an error or empty data.
- In live mode, `dirty::assess` reads `.LOG*` files with a plain open (no backup semantics); harmless for snapshot hives, could under-report on other users' unloaded hives.
- Hive log **replay** not implemented: dirty hives are detected (seq mismatch, or pending `HvLE` entries in .LOG1/.LOG2) and flagged; the base hive is read. Legacy (pre-8.1) `DIRT` logs are only flagged when seq numbers mismatch.
- No **real** Win10/Win11/Entra ID hives in fixtures yet. Need sanitized NTUSER.DAT + SOFTWARE + SYSTEM from test VMs (SPEC §13). Parser paths not yet exercised by fixtures: `lf`/`li`/`ri` subkey lists and `db` big-data values (code exists, unit tests only cover `lh`).
- `cfg(windows)` code is type-checked in the cloud only via `scripts/check-windows.sh` (std built from source); CI (`.github/workflows/ci.yml`) covers it once the repo is on GitHub.
- Whole hives are read into memory (SOFTWARE can be ~100 MB). Fine for now; consider mmap later.
- Built-in packs so far: chrome, edge, firefox, vscode, windows-terminal, powershell, dev-dotfiles, notepadpp, windows-shell. The rest of the SPEC §6 v1 list (Brave, Thunderbird, Outlook PST + signatures, Office, JetBrains, Sticky Notes, FileZilla, PuTTY, WinSCP, OBS, Adobe, Steam, Minecraft, My Games, QuickBooks) is M9. Packs have only been checked against synthetic fixtures; verify paths on real Win10/Win11 VMs.
- Packs can't name HKLM keys. `requires_closed` and `post_restore` are recorded only (used in live mode / restore). `rewrite: true` is metadata for M5.
- `.reg` export doesn't wrap long hex lines like regedit does; regedit accepts that. Values containing secrets aren't filtered (the allowlist keys hold none).
- `rebootly inspect` doesn't show packs yet; `backup --preflight` does.
- Target filesystem type: Linux (/proc/self/mountinfo) and live Windows (`GetVolumeInformationW`, passed in by the CLI). WinPE uses the Windows binary, so it is covered too once tested.
- Large backups: the manifest holds every file in memory and is written as one JSON file (~200 B per file). Fine up to a few hundred thousand files; consider streaming it later.
- No `report.html` yet (M6), only `report.txt`.
- SMART check and the `ddrescue` suggestion (SPEC §3.5) aren't implemented; they need platform code (M4/M7).
- `cargo-deny` config not added yet (M9, or earlier once there are more deps).

- Restore: `min_app_version` gating not implemented (`os:` is); file ACLs and read-only attributes aren't restored; `post_restore` commands aren't run (listed). Packs whose app is running are skipped as a whole. Long-running `winget import` output goes straight to the console (2 h timeout).
- Restore of other users' data needs a sign-in as each of them (by design); custom folders are restored by every run (identical the second time).
- Undo can't put back a Wi-Fi profile or certificate that the restore *replaced* (existing ones are never replaced, so this shouldn't happen).

## Crate map (one line per module: what it does)
- `rebootly-core::hive` — read-only regf parser: keys, values, case-insensitive lookup, bounds-checked (`mod.rs`, `cell.rs`, `value.rs`).
- `rebootly-core::hive::dirty` — dirty-hive detection from base block and .LOG/.LOG1/.LOG2.
- `rebootly-core::hive::writer` — minimal hive writer for building test fixtures only.
- `rebootly-core::env` — environment detection (live Windows / WinPE / Linux).
- `rebootly-core::expand` — case-insensitive `%VAR%` expansion with loop guard.
- `rebootly-core::winpath` — Windows path parsing; mapping onto a mounted volume with case-insensitive, no-symlink resolution.
- `rebootly-core::install` — find Windows installs on volumes; OS info (build, edition, computer name) from SOFTWARE/SYSTEM.
- `rebootly-core::profiles` — ProfileList parsing, account kind, machine-level variables.
- `rebootly-core::folders` — KNOWNFOLDERID table; User Shell Folders → Shell Folders → default resolution; per-user variables.
- `rebootly-core::source` — `Source` trait and `OfflineVolume` implementation (profiles, folders, hive health).
- `rebootly-core::inspect` — serialisable inspect report.
- `rebootly-core::backup::exclude` — §3.6 always-exclusions, categorised; AppData-scoped rules.
- `rebootly-core::backup::walk` — folder walker: sorted, no link traversal, records reparse points and name issues.
- `rebootly-core::backup::copy` — streaming copy with SHA-256, retry/backoff, `.rbpart` + rename, mtime kept.
- `rebootly-core::backup::plan` — users × Tier 1 folders in priority order, raw hives, KNOWNFOLDERID map, manual steps.
- `rebootly-core::backup::preflight` — sizes per folder, free space, FAT32 4 GiB, name issues, source-volume guard.
- `rebootly-core::backup::state` — `job.state` JSONL writer/loader and resume check.
- `rebootly-core::backup::run` — job folder create/resume, bounded parallel copy, manifest + report.
- `rebootly-core::backup::{manifest,report,time}` — §10 manifest types, report.txt, RFC 3339 formatting.
- `rebootly-core::backup::extras` — per-user pack detection and planning (copy, list_only, sensitive, .reg export).
- `rebootly-core::backup::custom` — custom folders, `Users\Public`, drive-root folder discovery.
- `rebootly-core::packs::{schema,load,detect,glob}` — pack YAML schema and validation, built-in + folder loading, detection, glob matcher.
- `rebootly-core::apps` — installed apps from Uninstall keys (machine 64/32-bit, per user).
- `rebootly-core::regfile` — export HKCU keys to `.reg` (regedit 5.00, UTF-16LE).
- `packs/*.yaml` — built-in packs (also compiled into the binary).
- `rebootly-core::fixtures` — synthetic `win11-basic` volume (feature `fixtures`); `examples/gen_fixtures.rs` regenerates it.
- `rebootly-cli` — `rebootly inspect …` (`render.rs`) and `rebootly backup --windows-root <p> | --live --target <dir> | --resume <job> [--user] [--folder] [--preflight] [--force] [--jobs N] [--sensitive] [--cloud-files skip|hydrate] [--keep-zone-identifier]` (`backup.rs`); `rebootly pack list | validate FILE…` (`pack.rs`).
- `rebootly-core::source::access` — `VolumeAccess` strategy (`OfflineAccess`), `HiveRef`, `LoadedHive`.
- `rebootly-core::backup::system` — Tier 4 file items and live captures into the plan.
- `rebootly-core::backup::sensitive` — `sensitive.age` write/list/extract, `Passphrase`.
- `rebootly-core::backup::filecopy` — one file: main stream with the locked-file snapshot fallback, then alternate data streams.
- `rebootly-win::live` (cfg(windows)) — `LiveWindows` (`mod.rs`), `access` (backup-semantics reads, snapshots), `regsave` (RegSaveKeyExW), `token` (elevation, privileges, SID), `capture/` (`inventory`: winget, Get-Printer, powercfg, dism, pnputil; `secrets`: netsh Wi-Fi, certificates), `cmd` (timeouts, PowerShell -EncodedCommand), `vss` (shadow copies), `streams` (ADS), `volume` (volume root, filesystem type), `knownfolder`, `power` (SleepGuard, on_battery), `wide`.
- `rebootly-win::parse` — platform-neutral parsing for the live code (stream info buffers, GUIDs, VSS answers, base64), tested on Linux.
- `rebootly-cli` also: `source.rs` (`--live` / `--windows-root`, keep-awake), `sensitive.rs` (passphrase input, `rebootly sensitive list|extract`).
- `rebootly-core::restore` — restore engine (SPEC §5): `input` (manifest view, decrypted bundle in a private temp dir), `target` (new PC/user, path mapping, `--simulate-root`), `map` (backup file → destination), `plan` + `regplan` + `notes` + `system` (phase 1: files, `.reg` checks, Tier 4 actions, manual steps), `run` + `phases` + `files` (phases 2–9), `conflict` (policies, `(migrated)` names), `rewrite` (profile paths in text), `regcheck` (.reg allowlist check), `merge` (hosts, phonebooks), `undo` (journal, `undo`, `list`), `host` (`RestoreHost`, `SysOp`, `SimHost`), `report` (plan/result text).
- `rebootly-win::live::restore` (cfg(windows)) — `live_target()` (current user's known folders, env, build) and `WinRestoreHost` (reg, netsh, powercfg, PowerShell certificates, schtasks, winget, tasklist, streams); `attrs` (hidden/system attributes).
- `rebootly-cli` also: `restore.rs` (`rebootly restore`, prompts), `undo.rs` (`rebootly undo`).
- `rebootly-linux`, `rebootly-tui` — placeholders for M7, M6.
