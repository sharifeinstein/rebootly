# Kickoff prompt — paste this into a new chat and attach SPEC.md

---

You are the lead engineer on `migrate`: a portable Rust tool that backs up and migrates a Windows PC (files, app settings, shell settings, app inventory) to USB/NAS, and restores to a new PC. It runs in live Windows, WinPE and a small Linux recovery environment. The full specification is attached as **SPEC.md**. It is the source of truth. All the major decisions in it are settled (Rust, ratatui TUI first, pure-Rust offline hive parser in every mode, plain-folder backup format with a manifest, YAML app packs, read-only offline mounting, no credential decryption, Ventoy rescue USB). Don't re-argue them unless you find a real blocker. If you do, explain it and ask me.

## Setup (first session only)
1. Check the environment can build: `cargo --version`, and whether crates.io is reachable. If package downloads are blocked, stop and tell me. I'll need to enable network/package access, or we build on my Windows PC instead.
2. Ask me where the repo lives: GitHub repo name, a folder on my linked PC, or this workspace. Then create the workspace layout from SPEC §2.
3. Save the attached spec to `docs/SPEC.md` unchanged. Create `docs/PROGRESS.md` from the template below.
4. Start **M1** (SPEC §14).

## How we work (context discipline, important)
- **One milestone at a time.** At the start of each milestone, read only the SPEC sections it needs (e.g. M1 → §3.1–3.4, §10, §13). Don't re-read the whole spec.
- **Keep tool output small:** `cargo build -q`, `cargo test -q 2>&1 | tail -40`, `cargo clippy -q`. Never dump whole files or long logs into the chat; grep or read line ranges instead.
- **Small modules** (~300 lines max per file), so later edits only need to read small pieces.
- Write tests alongside the code. A milestone is done only when `cargo test` and `cargo clippy -- -D warnings` pass and the milestone's CLI command works on fixtures.
- **After every milestone:** update `docs/PROGRESS.md`, commit (if git), and give me a summary of 5 lines or fewer: what works, what's next, and any decisions you need from me.
- If you change a design decision, record it in the PROGRESS.md "Decisions" log **and** update SPEC.md, so the two never drift apart.
- When this chat is getting long (roughly after two milestones, or if you notice yourself re-reading things), tell me. I'll start a fresh chat with the resume prompt below.
- Windows-only code (VSS, KnownFolder API, privileges) goes behind `cfg(windows)`. Core logic must compile and test on Linux.
- No secrets in logs, ever. Never add code that decrypts DPAPI data or reads browser password/cookie stores.

## docs/PROGRESS.md template
```markdown
# PROGRESS
## Status
Current milestone: M1 — <state>
Last session: <date> — <one line>
## Next step
<the exact next task, specific enough to resume cold>
## Milestones
- [ ] M1 Core resolver
- [ ] M2 Copy engine
- [ ] M3 Packs
- [ ] M4 Live Windows
- [ ] M5 Restore
- [ ] M6 TUI
- [ ] M7 Offline (WinPE/Linux, BitLocker)
- [ ] M8 Rescue USB (Ventoy, Alpine, WinPE scripts)
- [ ] M9 Hardening & release
## Decisions log
- <date>: <decision> — <why>
## Known issues / TODO
-
## Crate map (one line per module: what it does)
-
```

---

# Resume prompt — use for every later chat

> Continuing the Rebootly project (portable Rust PC migration/backup tool). Repo: D:\Projects\rebootly on my PC (connected folder, git repo, no GitHub yet).
>
> Read docs/PROGRESS.md first: it has the "How we work" rules, the sync procedure and the exact next step. Then read only the SPEC.md sections needed for the current step.
>
> Build and test in your cloud workspace (copy the repo there; the VM behind my folder has no Rust). I can build, test and run on Windows myself from PowerShell: when something needs real Windows, give me the exact commands and I'll paste back the output. Don't spend effort on Windows tooling in the cloud. Ask once for delete permission on the repo folder before running git there. Sync back after each step using the procedure in PROGRESS.md.
>
> Pick up at "Next step".
