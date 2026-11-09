//! `job.state`: an append-only JSON-lines log that makes a backup resumable
//! after a cancel, crash or NAS dropout (SPEC §3.5).
//!
//! Line 1 is a header; every later line is a `FileEntry`. Each line is
//! flushed as it is written; a torn last line is ignored when loading.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::manifest::{FileEntry, FileStatus};

pub const STATE_FILE: &str = "job.state";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Header {
    pub job_id: String,
    pub schema_version: u32,
    pub started: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
enum Line {
    Header(Header),
    File(FileEntry),
}

pub struct StateWriter {
    file: File,
}

impl StateWriter {
    /// Create a new state file (fails if one exists) or open one for append.
    pub fn create(dir: &Path, header: &Header) -> io::Result<Self> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(dir.join(STATE_FILE))?;
        writeln!(
            file,
            "{}",
            serde_json::to_string(&Line::Header(header.clone()))?
        )?;
        file.sync_data()?;
        Ok(StateWriter { file })
    }

    pub fn append_to(dir: &Path) -> io::Result<Self> {
        let mut file = OpenOptions::new().append(true).open(dir.join(STATE_FILE))?;
        // Start on a fresh line in case the previous run was killed mid-line.
        file.write_all(b"\n")?;
        Ok(StateWriter { file })
    }

    pub fn record(&mut self, entry: &FileEntry) -> io::Result<()> {
        let line = serde_json::to_string(&Line::File(entry.clone()))?;
        writeln!(self.file, "{line}")?;
        self.file.flush()
    }

    pub fn sync(&mut self) -> io::Result<()> {
        self.file.sync_data()
    }
}

#[derive(Debug, Default)]
pub struct Loaded {
    pub header: Option<Header>,
    /// Latest entry per backup path.
    pub entries: HashMap<String, FileEntry>,
}

impl Loaded {
    /// True if `path` was copied before with this size/mtime and the copy is
    /// still on the target with the right size.
    pub fn already_done(
        &self,
        job_dir: &Path,
        path: &str,
        size: u64,
        mtime: &Option<String>,
    ) -> bool {
        let Some(e) = self.entries.get(path) else {
            return false;
        };
        e.status == FileStatus::Ok
            && e.size == size
            && &e.mtime == mtime
            && std::fs::metadata(job_dir.join(path)).is_ok_and(|m| m.len() == size)
    }
}

pub fn load(dir: &Path) -> io::Result<Loaded> {
    let mut out = Loaded::default();
    for line in BufReader::new(File::open(dir.join(STATE_FILE))?).lines() {
        let line = line?;
        match serde_json::from_str::<Line>(&line) {
            Ok(Line::Header(h)) if out.header.is_none() => out.header = Some(h),
            Ok(Line::File(f)) => {
                out.entries.insert(f.path.clone(), f);
            }
            _ => {} // blank, torn or duplicate header: ignore
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, status: FileStatus) -> FileEntry {
        FileEntry {
            path: path.into(),
            source: r"C:\x".into(),
            size: 3,
            mtime: Some("2026-01-01T00:00:00Z".into()),
            attributes: None,
            sha256: Some("ab".into()),
            status,
            reason: None,
            snapshot: false,
            streams: Vec::new(),
        }
    }

    #[test]
    fn roundtrip_with_torn_tail_and_resume_check() {
        let t = tempfile::tempdir().unwrap();
        let h = Header {
            job_id: "j1".into(),
            schema_version: 1,
            started: "s".into(),
        };
        let mut w = StateWriter::create(t.path(), &h).unwrap();
        w.record(&entry("a", FileStatus::Failed)).unwrap();
        w.record(&entry("a", FileStatus::Ok)).unwrap();
        w.record(&entry("b", FileStatus::Ok)).unwrap();
        drop(w);
        assert!(
            StateWriter::create(t.path(), &h).is_err(),
            "never clobbers a state file"
        );
        // Simulate a crash mid-line, then a resumed run appending.
        let mut f = OpenOptions::new()
            .append(true)
            .open(t.path().join(STATE_FILE))
            .unwrap();
        f.write_all(br#"{"t":"file","path":"c","sour"#).unwrap();
        drop(f);
        let mut w = StateWriter::append_to(t.path()).unwrap();
        w.record(&entry("d", FileStatus::Ok)).unwrap();
        drop(w);

        let l = load(t.path()).unwrap();
        assert_eq!(l.header.as_ref().unwrap().job_id, "j1");
        assert_eq!(l.entries.len(), 3);
        assert_eq!(l.entries["a"].status, FileStatus::Ok);

        std::fs::write(t.path().join("a"), b"abc").unwrap();
        let m = Some("2026-01-01T00:00:00Z".to_owned());
        assert!(l.already_done(t.path(), "a", 3, &m));
        assert!(!l.already_done(t.path(), "a", 4, &m), "size changed");
        assert!(
            !l.already_done(t.path(), "b", 3, &m),
            "copy missing on target"
        );
        assert!(!l.already_done(t.path(), "a", 3, &None), "mtime changed");
    }
}
