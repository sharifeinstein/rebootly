//! Plain-text output of `verify` and `compare`.

use std::fmt::Write as _;

use super::compare::{CompareReport, State};
use super::verify::{BundleCheck, Problem, VerifyReport};
use crate::backup::preflight::gib;

/// Lines per list unless the caller asks for all.
pub const DEFAULT_LIMIT: usize = 50;

fn more(out: &mut String, shown: usize, total: usize) {
    if total > shown {
        let _ = writeln!(out, "  … and {} more (--all lists them all)", total - shown);
    }
}

pub fn verify_text(r: &VerifyReport, limit: usize) -> String {
    let mut o = String::new();
    let _ = writeln!(o, "Verify: {} (job {})", r.backup, r.job_id);
    let streams = match r.streams_checked {
        0 => String::new(),
        n => format!(" and {n} stream(s)"),
    };
    let _ = writeln!(
        o,
        "Checked {} file(s){streams}, {}.",
        r.files_checked,
        gib(r.bytes_checked)
    );
    if !r.complete {
        o.push_str(
            "The backup is incomplete (it was interrupted): finish it with `rebootly-cli backup --resume`.\n",
        );
    }
    if r.problems.is_empty() {
        o.push_str("Every copied file matches the manifest.\n");
    } else {
        let _ = writeln!(o, "Problems ({}):", r.problems.len());
        for f in r.problems.iter().take(limit) {
            let what = match &f.problem {
                Problem::Missing => "missing".to_owned(),
                Problem::Size { expected, found } => {
                    format!("wrong size (expected {expected} B, found {found} B)")
                }
                Problem::Hash => "content changed (SHA-256 differs)".to_owned(),
                Problem::Unreadable { error } => format!("can't be read: {error}"),
                Problem::UnsafePath => "path outside the backup folder (not checked)".to_owned(),
            };
            let _ = writeln!(o, "  {}: {what}", f.path);
        }
        more(&mut o, limit.min(r.problems.len()), r.problems.len());
    }
    if r.failed_in_backup > 0 {
        let _ = writeln!(
            o,
            "Not checked: {} file(s) the backup couldn't copy (see report.txt).",
            r.failed_in_backup
        );
    }
    if r.not_hashed > 0 {
        let _ = writeln!(
            o,
            "{} file(s) have no recorded hash: only their size was checked.",
            r.not_hashed
        );
    }
    let _ = match &r.bundle {
        BundleCheck::None => Ok(()),
        BundleCheck::Missing { file } => writeln!(o, "Encrypted bundle {file}: MISSING."),
        BundleCheck::NotOpened { file, size } => writeln!(
            o,
            "Encrypted bundle {file}: there ({}), not opened; add --sensitive to check it.",
            gib(*size)
        ),
        BundleCheck::Ok { file, files } => {
            writeln!(
                o,
                "Encrypted bundle {file}: opened, {files} file(s), all match."
            )
        }
        BundleCheck::Failed { file, error } => {
            writeln!(o, "Encrypted bundle {file}: FAILED: {error}")
        }
    };
    if !r.extra.is_empty() {
        let _ = writeln!(
            o,
            "Files in the folder that the manifest doesn't list ({}; not a problem in itself):",
            r.extra.len()
        );
        for e in r.extra.iter().take(limit.min(10)) {
            let _ = writeln!(o, "  {e}");
        }
        more(&mut o, limit.min(10).min(r.extra.len()), r.extra.len());
    }
    o.push_str(if r.is_good() {
        "Result: OK\n"
    } else {
        "Result: PROBLEMS FOUND\n"
    });
    o
}

/// Summary order and wording, per state.
const LINES: [(State, &str); 11] = [
    (State::Same, "same as in the backup"),
    (
        State::Rewritten,
        "same, with the old profile path rewritten",
    ),
    (
        State::Beside,
        "kept beside a different file of the same name",
    ),
    (State::Merged, "merged into this PC's file (not compared)"),
    (State::KeptOwn, "this PC's own shortcut or desktop.ini kept"),
    (State::LeftOut, "left out by the restore, by design"),
    (State::Newer, "changed on this PC since the backup"),
    (State::Different, "DIFFERENT"),
    (State::Missing, "MISSING"),
    (State::Blocked, "a folder or link is in the way"),
    (State::Unreadable, "can't be read"),
];

/// States listed file by file, with their headings.
const LISTED: [(State, &str); 6] = [
    (State::Missing, "Missing"),
    (State::Different, "Different from the backup"),
    (State::Blocked, "A folder or link is in the way"),
    (State::Unreadable, "Can't be read"),
    (State::Newer, "Changed on this PC since the backup"),
    (State::LeftOut, "Left out by the restore, by design"),
];

pub fn compare_text(r: &CompareReport, limit: usize) -> String {
    let mut o = String::new();
    let _ = writeln!(o, "Compare: {} (job {})", r.backup, r.job_id);
    let _ = writeln!(
        o,
        "Backup user {} → {} ({}): {} file(s), {}.",
        r.user,
        r.target_user,
        r.target_profile,
        r.entries.len(),
        gib(r.bytes)
    );
    for (s, label) in LINES {
        let n = r.count(s);
        if n > 0 {
            let _ = writeln!(o, "  {n:>7}  {label}");
        }
    }
    for (s, heading) in LISTED {
        let all: Vec<_> = r.entries.iter().filter(|e| e.state == s).collect();
        if all.is_empty() {
            continue;
        }
        let _ = writeln!(o, "{heading} ({}):", all.len());
        for e in all.iter().take(limit) {
            match &e.detail {
                Some(d) => {
                    let _ = writeln!(o, "  {} ({d})", e.path);
                }
                None => {
                    let _ = writeln!(o, "  {}", e.path);
                }
            }
        }
        more(&mut o, limit.min(all.len()), all.len());
    }
    if !r.not_planned.is_empty() {
        let _ = writeln!(
            o,
            "Not part of the restore plan ({}), e.g.:",
            r.not_planned.len()
        );
        for (what, why) in r.not_planned.iter().take(5) {
            let _ = writeln!(o, "  {what}: {why}");
        }
    }
    if !r.other_users.is_empty() {
        let names: Vec<String> = r
            .other_users
            .iter()
            .map(|(u, n)| format!("{u} ({n} file(s))"))
            .collect();
        let _ = writeln!(
            o,
            "Other users in the backup, not compared: {} (run compare as each of them, with --user NAME).",
            names.join(", ")
        );
    }
    o.push_str("Settings (registry, Wi-Fi, power plan, certificates, apps) aren't compared.\n");
    o.push_str(if r.is_good() {
        "Result: OK\n"
    } else {
        "Result: DIFFERENCES FOUND\n"
    });
    o
}
