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
- Sync to D: (until GitHub is set up): `git gc -q && tar --exclude=./target --exclude=./.git/objects/info/commit-graph --mode=u+w -cf /mnt/user-data/outputs/rebootly-sync-<sha>.tar .`, device_commit_files it to `D:\Projects\rebootly\.rebootly-sync.tar` (use a new staged file name each time, or a stale copy is sent), then on the device `tar --overwrite -xf .rebootly-sync.tar`. Always keep `--mode=u+w`: a file that arrives read-only on Windows can't be overwritten from the VM (the user would have to run `attrib -r /s`).
- To resume in a new chat: copy `D:\Projects\rebootly` into the workspace (stage `.rebootly-sync.tar` after re-bundling on the device, or the files) (or clone from GitHub once it exists), then `cargo test`.

## Status
Current milestone: M2 — done; M3 not started
Last session: 2026-11-09 — M2 copy engine and `rebootly backup` (Tier 1 + raw hives, resume, pre-flight)

## Next step
Start M3 (packs, SPEC §6 and §4 Tier 2/3). First task: `crates/rebootly-core/src/packs/schema.rs`, serde structs for the YAML pack format from SPEC §6, with validation and tests, plus a loader for `packs/*.yaml`. Then detection against a `Source`, and copying pack paths through the existing engine: add `FolderJob`s with `dest_root = users/<u>/packs/<id>`, priority 13–19 (between Pictures and Favorites). The AppData exclusions in `backup/exclude.rs` already apply when `profile_rel` is set.

## Milestones
- [x] M1 Core resolver
- [x] M2 Copy engine
- [ ] M3 Packs
- [ ] M4 Live Windows
- [ ] M5 Restore
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

## Known issues / TODO
- Hive log **replay** not implemented: dirty hives are detected (seq mismatch, or pending `HvLE` entries in .LOG1/.LOG2) and flagged; the base hive is read. Legacy (pre-8.1) `DIRT` logs are only flagged when seq numbers mismatch.
- No **real** Win10/Win11/Entra ID hives in fixtures yet. Need sanitized NTUSER.DAT + SOFTWARE + SYSTEM from test VMs (SPEC §13). Parser paths not yet exercised by fixtures: `lf`/`li`/`ri` subkey lists and `db` big-data values (code exists, unit tests only cover `lh`).
- `cfg(windows)` code is not compile-checked in the cloud workspace (Windows target can't be installed there); CI (`.github/workflows/ci.yml`) covers it once the repo is on GitHub.
- Whole hives are read into memory (SOFTWARE can be ~100 MB). Fine for now; consider mmap later.
- `Source::installed_apps` not on the trait yet (comes with M3/M4).
- Tier 1 still missing: `Users\Public\*` (ask) and non-system folders at the drive root (e.g. `C:\Projects`). Plan: add both as "custom" FolderJobs in M3, which also adds custom folders.
- Target filesystem type is detected on Linux only (/proc/self/mountinfo). On Windows/WinPE it is unknown, so the FAT32 warning is generic there; rebootly-win should supply it in M4.
- Large backups: the manifest holds every file in memory and is written as one JSON file (~200 B per file). Fine up to a few hundred thousand files; consider streaming it later.
- No `report.html` yet (M6), only `report.txt`.
- SMART check and the `ddrescue` suggestion (SPEC §3.5) aren't implemented; they need platform code (M4/M7).
- `cargo-deny` config not added yet (M9, or earlier once there are more deps).

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
- `rebootly-core::fixtures` — synthetic `win11-basic` volume (feature `fixtures`); `examples/gen_fixtures.rs` regenerates it.
- `rebootly-cli` — `rebootly inspect …` (`render.rs`) and `rebootly backup --windows-root <p> --target <dir> | --resume <job> [--user] [--folder] [--preflight] [--force] [--jobs N]` (`backup.rs`).
- `rebootly-win`, `rebootly-linux`, `rebootly-tui` — placeholders for M4, M7, M6.
