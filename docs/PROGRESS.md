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
- Canonical tree: `D:\Projects\Migrateit` on the user's PC (repo root, with `.git`). GitHub: `sharifeinstein/rebootly` once the user creates it (the Claude session can't create repos).
- Builds run in the Claude cloud workspace (Rust 1.97, crates.io reachable); the tree is mirrored to D: after each milestone. The VM behind the linked folder has git but no Rust.
- Sync to D: (until GitHub is set up): `git gc -q && tar --exclude=./target --exclude=./.git/objects/info/commit-graph --mode=u+w -cf /mnt/user-data/outputs/rebootly-sync-<sha>.tar .`, device_commit_files it to `D:\Projects\Migrateit\.rebootly-sync.tar` (use a new staged file name each time, or a stale copy is sent), then on the device `tar --overwrite -xf .rebootly-sync.tar`. Files that arrive read-only on Windows can't be overwritten from the VM; the user clears them with `attrib -r /s`.
- To resume in a new chat: copy `D:\Projects\Migrateit` into the workspace (or clone from GitHub once it exists), then `cargo test`.

## Status
Current milestone: M1 — done; M2 not started
Last session: 2026-11-09 — setup, own hive parser, ProfileList + User Shell Folders resolver, `rebootly inspect`

## Next step
Start M2 (copy engine, SPEC §3.5, §3.6, §10, §13). First task: add `crates/rebootly-core/src/manifest.rs` (schema-versioned serde structs for §10, incl. per-file entry and KNOWNFOLDERID→path map, which `inspect::UserReport` already produces), then `copy/walk.rs`: a priority-ordered walker over `ResolvedFolder`s that records symlinks/junctions instead of following them and applies §3.6 exclusions. Then the `backup --windows-root <p> --target <dir>` CLI for Tier 1 only.

## Milestones
- [x] M1 Core resolver
- [ ] M2 Copy engine
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

- 2026-11-09: Project named **Rebootly** (was working name `migrate`, taken on crates.io). Crates `rebootly-*`, binary `rebootly`, markers `.rebootly-root` / `.rebootly-undo`. SPEC updated. The local folder is still `D:\Projects\Migrateit`.

## Known issues / TODO
- Hive log **replay** not implemented: dirty hives are detected (seq mismatch, or pending `HvLE` entries in .LOG1/.LOG2) and flagged; the base hive is read. Legacy (pre-8.1) `DIRT` logs are only flagged when seq numbers mismatch.
- No **real** Win10/Win11/Entra ID hives in fixtures yet. Need sanitized NTUSER.DAT + SOFTWARE + SYSTEM from test VMs (SPEC §13). Parser paths not yet exercised by fixtures: `lf`/`li`/`ri` subkey lists and `db` big-data values (code exists, unit tests only cover `lh`).
- `cfg(windows)` code is not compile-checked in the cloud workspace (Windows target can't be installed there); CI (`.github/workflows/ci.yml`) covers it once the repo is on GitHub.
- Whole hives are read into memory (SOFTWARE can be ~100 MB). Fine for now; consider mmap later.
- `Source::installed_apps` not on the trait yet (comes with M3/M4).
- Tier 1 items not resolved yet: `Users\Public\*` contents and non-system drive-root folders (e.g. `C:\Projects`) — M2 pre-flight.
- `cargo-deny` config not added yet (M9, or earlier once there are more deps).
- Root `SPEC.md` and `KICKOFF_PROMPT.md` in D:\Projects\Migrateit are git-ignored kickoff copies; `docs/SPEC.md` is canonical. Delete them when convenient.

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
- `rebootly-core::fixtures` — synthetic `win11-basic` volume (feature `fixtures`); `examples/gen_fixtures.rs` regenerates it.
- `rebootly-cli` — `rebootly inspect --windows-root <p> | --scan <dir>… [--json]`; `render.rs` text output.
- `rebootly-win`, `rebootly-linux`, `rebootly-tui` — placeholders for M4, M7, M6.
