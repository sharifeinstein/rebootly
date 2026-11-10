//! Plain-text plan (shown before a restore and by `--dry-run`) and result
//! (printed and saved as `report.txt` in the undo folder).

use std::collections::BTreeMap;
use std::fmt::Write;

use super::conflict::Category;
use super::plan::RestorePlan;
use super::run::{RestoreResult, Status};
use crate::backup::preflight::gib;

pub fn plan_text(p: &RestorePlan) -> String {
    let mut s = String::new();
    let t = &p.target;
    let _ = writeln!(
        s,
        "Restore {} from {} into {} ({}) on {}, {}",
        p.user.username,
        p.source_machine.as_deref().unwrap_or("the old PC"),
        t.username,
        t.profile,
        t.machine.as_deref().unwrap_or("this PC"),
        t.os_label()
    );
    if !p.user.profile_path.eq_ignore_ascii_case(&t.profile) {
        let _ = writeln!(s, "  old profile {} → {}", p.user.profile_path, t.profile);
    }
    if !p.folder_map.is_empty() {
        let _ = writeln!(s, "Folders:");
        for (name, old, new) in &p.folder_map {
            if name.ends_with("AppData") {
                continue;
            }
            let _ = writeln!(s, "  {name:<12} {old} → {new}");
        }
    }
    let _ = writeln!(s, "Files ({} total):", gib(p.bytes));
    for c in Category::ALL {
        let files: Vec<_> = p.files.iter().filter(|f| f.category == c).collect();
        if files.is_empty() {
            continue;
        }
        let bytes: u64 = files.iter().map(|f| f.size).sum();
        let _ = writeln!(
            s,
            "  {:<8} {:>6} files {:>12}   if it exists: {}",
            c.as_str(),
            files.len(),
            gib(bytes),
            p.policies.get(c).as_str()
        );
    }
    for pk in &p.packs {
        let closed = if pk.requires_closed.is_empty() {
            String::new()
        } else {
            format!(" (close {} first)", pk.requires_closed.join(", "))
        };
        let _ = writeln!(s, "  app: {} — {} files{closed}", pk.name, pk.files);
    }
    if !p.registry.is_empty() {
        let _ = writeln!(s, "Registry (HKCU, allowlisted keys only):");
        for r in &p.registry {
            let _ = writeln!(s, "  {}: {} keys", r.from, r.keys.len());
            for k in &r.review {
                let _ = writeln!(s, "    review: {k}");
            }
        }
    }
    if p.apps.is_some() || !p.system.is_empty() {
        let _ = writeln!(s, "System:");
        for a in p.apps.iter().chain(&p.system) {
            let _ = writeln!(s, "  {}", a.describe());
        }
    }
    skipped_summary(&mut s, p);
    for w in &p.warnings {
        let _ = writeln!(s, "warning: {w}");
    }
    for b in &p.blockers {
        let _ = writeln!(s, "BLOCKER: {b}");
    }
    s
}

fn skipped_summary(s: &mut String, p: &RestorePlan) {
    if p.skipped.is_empty() {
        return;
    }
    let mut by: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for k in &p.skipped {
        by.entry(&k.reason).or_default().push(&k.what);
    }
    let _ = writeln!(s, "Not restored:");
    for (reason, whats) in by {
        let ex = whats.iter().take(3).copied().collect::<Vec<_>>().join(", ");
        let more = if whats.len() > 3 {
            format!(", +{} more", whats.len() - 3)
        } else {
            String::new()
        };
        let _ = writeln!(s, "  {} item(s): {reason} ({ex}{more})", whats.len());
    }
}

pub fn result_text(p: &RestorePlan, r: &RestoreResult) -> String {
    let mut s = String::new();
    let head = if r.dry_run {
        "Dry run: nothing was changed."
    } else {
        "Restore finished."
    };
    let _ = writeln!(
        s,
        "{head} Backup {} → {} ({})",
        p.job_id, p.target.username, p.target.profile
    );
    let n = |st| r.count(st);
    if r.dry_run {
        let _ = writeln!(
            s,
            "  would change {}, unchanged/skipped {}",
            n(Status::Would),
            n(Status::Skipped)
        );
    } else {
        let _ = writeln!(
            s,
            "  done {}, skipped {}, failed {}; verified {} file(s){}",
            n(Status::Done),
            n(Status::Skipped),
            n(Status::Failed),
            r.verified,
            if r.verify_failed.is_empty() {
                String::new()
            } else {
                format!(", {} differ", r.verify_failed.len())
            }
        );
    }
    if let Some(why) = &r.stopped {
        let _ = writeln!(s, "  {why}");
    }
    if let Some(d) = &r.undo_dir {
        let _ = writeln!(s, "  undo: rebootly-cli undo \"{}\"", d.display());
    }
    let mut phase = None;
    for o in &r.outcomes {
        if Some(o.phase) != phase {
            phase = Some(o.phase);
            let _ = writeln!(s, "\n{}:", o.phase.label());
        }
        let tag = match o.status {
            Status::Done => "ok  ",
            Status::Skipped => "skip",
            Status::Failed => "FAIL",
            Status::Would => "plan",
        };
        let detail = if o.detail.is_empty() {
            String::new()
        } else {
            format!(" — {}", o.detail)
        };
        let _ = writeln!(s, "  {tag} {}{detail}", o.what);
    }
    for v in &r.verify_failed {
        let _ = writeln!(s, "  VERIFY FAILED {v}");
    }
    if !r.manual_steps.is_empty() {
        let _ = writeln!(s, "\nManual steps:");
        for m in &r.manual_steps {
            let _ = writeln!(s, "  [ ] {} — {}", m.title, m.detail);
        }
    }
    s
}
