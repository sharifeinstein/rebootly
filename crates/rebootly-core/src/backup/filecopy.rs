//! Back up one planned file: the main stream (with the locked-file snapshot
//! fallback), then its alternate data streams (SPEC §3.5).
//!
//! Streams are stored as separate files under `streams/<dest>/<name>`, since
//! targets like exFAT or a NAS can't hold them; restore (M5) writes them back.
//! `Zone.Identifier` (the "downloaded from the internet" mark) is dropped
//! unless `keep_zone_identifier`.

use std::sync::atomic::AtomicBool;

use super::copy::{RetryPolicy, copy_file};
use super::manifest::{FileEntry, FileStatus, StreamEntry};
use super::run::Job;
use super::time::rfc3339;
use super::walk::PlannedFile;
use crate::source::Source;

/// Streams above this size are listed but not stored.
pub const MAX_STREAM: u64 = 64 << 20;
pub const STREAMS_DIR: &str = "streams";
const ZONE_IDENTIFIER: &str = "Zone.Identifier";

#[derive(Debug, Clone, Copy)]
pub struct FileCopyOptions {
    pub retry: RetryPolicy,
    pub keep_zone_identifier: bool,
}

pub fn entry(
    f: &PlannedFile,
    status: FileStatus,
    sha256: Option<String>,
    reason: Option<String>,
) -> FileEntry {
    FileEntry {
        path: f.dest.clone(),
        source: f.src_win.clone(),
        size: f.size,
        mtime: f.mtime.map(rfc3339),
        attributes: f.attributes,
        sha256,
        status,
        reason,
        snapshot: false,
        streams: Vec::new(),
    }
}

/// `None` when cancelled.
pub fn copy_one(
    src: &dyn Source,
    f: &PlannedFile,
    job: &Job,
    opts: FileCopyOptions,
    cancel: &AtomicBool,
) -> Option<FileEntry> {
    let open = |snap: bool| {
        if snap {
            src.open_snapshot(&f.src)
        } else {
            src.open_read(&f.src)
        }
    };
    let c = match copy_file(&open, &job.native(&f.dest), f.mtime, opts.retry, cancel) {
        Ok(c) => c,
        Err(fail) if fail.reason == "cancelled" => return None,
        Err(fail) => {
            let reason = format!("{} (after {} attempts)", fail.reason, fail.attempts);
            return Some(entry(f, FileStatus::Failed, None, Some(reason)));
        }
    };
    let mut e = entry(f, FileStatus::Ok, Some(c.sha256), None);
    e.snapshot = c.from_snapshot;
    match src.alt_streams(&f.src, c.from_snapshot) {
        Ok(list) => {
            let mut used = std::collections::HashSet::new();
            for s in list {
                if !opts.keep_zone_identifier && s.name.eq_ignore_ascii_case(ZONE_IDENTIFIER) {
                    continue;
                }
                let name = unique(stream_file_name(&s.name), &mut used);
                let stored = format!("{STREAMS_DIR}/{}/{name}", f.dest);
                e.streams.push(copy_stream(
                    src,
                    f,
                    &s,
                    stored,
                    c.from_snapshot,
                    job,
                    opts,
                    cancel,
                )?);
            }
        }
        Err(err) => e.reason = Some(format!("alternate data streams not checked: {err}")),
    }
    Some(e)
}

#[allow(clippy::too_many_arguments)]
fn copy_stream(
    src: &dyn Source,
    f: &PlannedFile,
    s: &crate::StreamInfo,
    stored: String,
    snap: bool,
    job: &Job,
    opts: FileCopyOptions,
    cancel: &AtomicBool,
) -> Option<StreamEntry> {
    let mut out = StreamEntry {
        name: s.name.clone(),
        size: s.size,
        path: None,
        sha256: None,
        status: FileStatus::Skipped,
        reason: None,
    };
    if s.size > MAX_STREAM {
        out.reason = Some(format!("larger than {} MiB, not stored", MAX_STREAM >> 20));
        return Some(out);
    }
    // Same source as the main stream: a locked file's streams come from the
    // snapshot too.
    let open = |_| src.open_stream(&f.src, &s.name, snap);
    match copy_file(&open, &job.native(&stored), None, opts.retry, cancel) {
        Ok(c) => {
            out.status = FileStatus::Ok;
            out.sha256 = Some(c.sha256);
            out.size = c.bytes;
            out.path = Some(stored);
        }
        Err(fail) if fail.reason == "cancelled" => return None,
        Err(fail) => {
            out.status = FileStatus::Failed;
            out.reason = Some(fail.reason);
        }
    }
    Some(out)
}

/// A stream name as one safe path component (never `.`/`..`).
fn stream_file_name(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | ' ' | '{' | '}') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if s.trim_matches('.').is_empty() {
        format!("_{s}")
    } else {
        s
    }
}

fn unique(name: String, used: &mut std::collections::HashSet<String>) -> String {
    let mut n = name.clone();
    let mut i = 2;
    while !used.insert(n.to_lowercase()) {
        n = format!("{name}~{i}");
        i += 1;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_names_are_safe_components() {
        assert_eq!(stream_file_name("Zone.Identifier"), "Zone.Identifier");
        assert_eq!(stream_file_name("a/b\\c:d"), "a_b_c_d");
        assert_eq!(stream_file_name(".."), "_..");
        let mut used = Default::default();
        assert_eq!(unique("x".into(), &mut used), "x");
        assert_eq!(unique("X".into(), &mut used), "X~2");
    }
}
