//! Dirty-hive detection (SPEC §3.4).
//!
//! A hive is "dirty" when its base block sequence numbers differ, or when a
//! transaction log (`.LOG1` / `.LOG2`) holds entries not yet applied to the
//! primary file. Log *replay* is not implemented yet; callers read the base
//! hive and flag the result in the report.

use std::path::{Path, PathBuf};

use serde::Serialize;

use super::{Header, u32_at};

const LOG_BASE_LEN: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LogFormat {
    /// Missing, zero-filled, or shorter than a base block.
    Empty,
    /// Windows 8.1+ format: base block followed by `HvLE` entries.
    New,
    /// Pre-8.1 format: base block followed by a `DIRT` dirty vector.
    Legacy,
    /// Has a base block but no recognisable body.
    Unrecognised,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LogInfo {
    pub path: PathBuf,
    pub size: u64,
    pub format: LogFormat,
    /// Sequence number of the newest applicable log entry (new format only).
    pub newest_pending_seq: Option<u32>,
    /// True if this log appears to hold changes missing from the base hive.
    pub pending: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HiveState {
    pub seq_mismatch: bool,
    pub checksum_ok: bool,
    pub logs: Vec<LogInfo>,
}

impl HiveState {
    pub fn is_dirty(&self) -> bool {
        self.seq_mismatch || self.logs.iter().any(|l| l.pending)
    }
}

/// Find `<hive>.LOG`, `.LOG1`, `.LOG2` beside the hive, case-insensitively.
pub fn log_paths(hive_path: &Path) -> Vec<PathBuf> {
    let (Some(dir), Some(name)) = (hive_path.parent(), hive_path.file_name()) else {
        return Vec::new();
    };
    let name = name.to_string_lossy().to_uppercase();
    let wanted = ["LOG", "LOG1", "LOG2"].map(|s| format!("{name}.{s}"));
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = rd
        .filter_map(|e| e.ok())
        .filter(|e| wanted.contains(&e.file_name().to_string_lossy().to_uppercase()))
        .map(|e| e.path())
        .collect();
    out.sort();
    out
}

/// Assess a hive given its parsed header and its path on disk.
pub fn assess(hive_path: &Path, header: &Header) -> HiveState {
    let logs = log_paths(hive_path)
        .into_iter()
        .map(|p| inspect_log(&p, header))
        .collect();
    HiveState {
        seq_mismatch: header.seq_mismatch(),
        checksum_ok: header.checksum_ok,
        logs,
    }
}

fn inspect_log(path: &Path, header: &Header) -> LogInfo {
    let data = std::fs::read(path).unwrap_or_default();
    let mut info = LogInfo {
        path: path.to_path_buf(),
        size: data.len() as u64,
        format: LogFormat::Empty,
        newest_pending_seq: None,
        pending: false,
    };
    if data.len() < LOG_BASE_LEN || &data[0..4] != b"regf" {
        return info;
    }
    match data.get(LOG_BASE_LEN..LOG_BASE_LEN + 4) {
        Some(b"HvLE") => {
            info.format = LogFormat::New;
            info.newest_pending_seq = newest_pending(&data, header.secondary_seq);
            info.pending = info.newest_pending_seq.is_some();
        }
        Some(b"DIRT") => {
            info.format = LogFormat::Legacy;
            // Legacy logs only matter when the base block is inconsistent.
            info.pending = header.seq_mismatch();
        }
        _ => info.format = LogFormat::Unrecognised,
    }
    info
}

/// Walk `HvLE` entries; entries with seq >= the hive's secondary seq, in an
/// unbroken ascending run, are the ones a replay would apply.
fn newest_pending(data: &[u8], hive_secondary: u32) -> Option<u32> {
    let mut pos = LOG_BASE_LEN;
    let mut expected: Option<u32> = None;
    let mut newest = None;
    while data.get(pos..pos + 4) == Some(b"HvLE") {
        let size = u32_at(data, pos + 4)? as usize;
        let seq = u32_at(data, pos + 12)?;
        if size < 40 || size % 512 != 0 || pos + size > data.len() {
            break;
        }
        match expected {
            None if seq >= hive_secondary => {}
            Some(e) if seq == e => {}
            None => {
                pos += size;
                continue;
            }
            Some(_) => break,
        }
        newest = Some(seq);
        expected = seq.checked_add(1);
        pos += size;
    }
    newest
}
