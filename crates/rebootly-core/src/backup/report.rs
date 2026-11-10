//! Human-readable `report.txt` (SPEC §10); `report_html` has the printable
//! version.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use super::manifest::{FileStatus, Manifest};
use super::preflight::gib;

pub(crate) const MAX_LISTED: usize = 50;

/// Copied files and bytes per row: `users/<u>/<folder>/<sub>` or
/// `system/<item>` (one row per kind, not per file). Streams are left out.
pub(crate) fn per_folder(m: &Manifest) -> BTreeMap<String, (u64, u64)> {
    let mut per_folder: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    for f in m.files.iter().filter(|f| f.status == FileStatus::Ok) {
        if f.path.starts_with("streams/") {
            continue;
        }
        let key: Vec<&str> = f.path.split('/').take(4).collect();
        let key = if key.first() == Some(&"users") {
            key.join("/")
        } else {
            key[..key.len().min(2)].join("/")
        };
        let e = per_folder.entry(key).or_default();
        e.0 += 1;
        e.1 += f.size;
    }
    per_folder
}

pub fn text(m: &Manifest) -> String {
    let mut s = String::new();
    let (ok, skipped, failed) = m.counts();
    let bytes: u64 = m
        .files
        .iter()
        .filter(|f| f.status == FileStatus::Ok)
        .map(|f| f.size)
        .sum();
    let _ = writeln!(s, "Rebootly backup report");
    let _ = writeln!(s, "======================");
    let _ = writeln!(
        s,
        "Source:   {} {} ({})",
        m.source.machine.as_deref().unwrap_or("?"),
        m.source.os_name,
        m.source.environment
    );
    let _ = writeln!(s, "Job:      {}", m.job_id);
    let _ = writeln!(s, "Started:  {}", m.started);
    let _ = writeln!(
        s,
        "Finished: {}",
        m.finished
            .as_deref()
            .unwrap_or("not finished (resume with --resume)")
    );
    let _ = writeln!(
        s,
        "Result:   {ok} copied ({}), {skipped} skipped, {failed} failed",
        gib(bytes)
    );

    let snap = m.files.iter().filter(|f| f.snapshot).count();
    if snap > 0 {
        let _ = writeln!(
            s,
            "          {snap} file(s) were open in another program and were read from a \
             shadow copy (their state when the copy was made)"
        );
    }
    let (streams, stream_fail) = m
        .files
        .iter()
        .flat_map(|f| &f.streams)
        .fold((0, 0), |a, st| {
            (
                a.0 + usize::from(st.status == FileStatus::Ok),
                a.1 + usize::from(st.status != FileStatus::Ok),
            )
        });
    if streams + stream_fail > 0 {
        let _ = writeln!(
            s,
            "          {streams} alternate data stream(s) kept under streams/, {stream_fail} not stored"
        );
    }

    let per_folder = per_folder(m);
    let _ = writeln!(s, "\nCopied by folder:");
    for (k, (n, b)) in &per_folder {
        let _ = writeln!(s, "  {k:<50} {n:>7} files  {}", gib(*b));
    }

    let failures: Vec<_> = m
        .files
        .iter()
        .filter(|f| f.status == FileStatus::Failed)
        .collect();
    if !failures.is_empty() {
        let _ = writeln!(s, "\nFailed files ({}):", failures.len());
        for f in failures.iter().take(MAX_LISTED) {
            let _ = writeln!(
                s,
                "  {}  -- {}",
                f.source,
                f.reason.as_deref().unwrap_or("?")
            );
        }
        if failures.len() > MAX_LISTED {
            let _ = writeln!(
                s,
                "  ... and {} more (see manifest.json)",
                failures.len() - MAX_LISTED
            );
        }
    }
    if let Some(sb) = &m.sensitive {
        let _ = writeln!(
            s,
            "\nEncrypted sensitive bundle: {} ({} file(s), {}).",
            sb.file,
            sb.files.len(),
            sb.format
        );
        let _ = writeln!(
            s,
            "  Open it with `rebootly-cli sensitive extract` or `age -d {} | tar -x`; \
             the passphrase is not stored anywhere.",
            sb.file
        );
        for f in &sb.failed {
            let _ = writeln!(s, "  not included: {} ({})", f.source, f.reason);
        }
    }
    if !m.excluded.is_empty() {
        let _ = writeln!(s, "\nLeft out on purpose:");
        for e in &m.excluded {
            let _ = writeln!(
                s,
                "  {:<60} {:>6} items  {}",
                e.description,
                e.count,
                gib(e.bytes)
            );
        }
    }
    if !m.reparse_points.is_empty() {
        let _ = writeln!(
            s,
            "\n{} link(s)/junction(s) recorded, not followed (listed in manifest.json).",
            m.reparse_points.len()
        );
    }
    if !m.warnings.is_empty() {
        let _ = writeln!(s, "\nWarnings:");
        for w in &m.warnings {
            let _ = writeln!(s, "  ! {w}");
        }
    }
    let _ = writeln!(s, "\nManual steps:");
    if m.manual_steps.is_empty() {
        let _ = writeln!(s, "  (none for the folders in this backup)");
    }
    for (i, step) in m.manual_steps.iter().enumerate() {
        let _ = writeln!(
            s,
            "  [ ] {}. {}\n        {}",
            i + 1,
            step.title,
            step.detail
        );
    }
    s
}
