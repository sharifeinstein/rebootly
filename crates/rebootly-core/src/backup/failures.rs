//! Failed files as the reports list them: files that share a backup folder
//! and a reason collapse into one line once there are [`GROUP_MIN`] of them
//! (a running Edge made 334 "Access is denied" lines on the laptop), plus a
//! plain-language hint per kind of error.

use super::manifest::{FileEntry, FileStatus};

/// Files with the same folder and reason that make one line.
pub const GROUP_MIN: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FailLine<'a> {
    File {
        source: &'a str,
        reason: &'a str,
    },
    /// `folder` is the deepest source folder all the files are in.
    Group {
        folder: String,
        reason: &'a str,
        count: usize,
    },
}

/// The report row a file belongs to (as in "Copied by folder"):
/// `users/<u>/<kind>/<name>` or `system/<item>`.
fn row(path: &str) -> &str {
    let take = if path.starts_with("users/") { 4 } else { 2 };
    match path.match_indices('/').nth(take - 1) {
        Some((i, _)) => &path[..i],
        None => path,
    }
}

fn reason(f: &FileEntry) -> &str {
    f.reason.as_deref().unwrap_or("?")
}

/// Failed files, in the order they failed; groups sit where their first
/// file was.
pub fn lines(files: &[FileEntry]) -> Vec<FailLine<'_>> {
    let failed: Vec<&FileEntry> = files
        .iter()
        .filter(|f| f.status == FileStatus::Failed)
        .collect();
    // (row, reason) → indexes into `failed`, in first-seen order.
    let mut groups: Vec<((&str, &str), Vec<usize>)> = Vec::new();
    for (i, f) in failed.iter().enumerate() {
        let key = (row(&f.path), reason(f));
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, v)) => v.push(i),
            None => groups.push((key, vec![i])),
        }
    }
    let mut out = Vec::new();
    for ((_, why), idx) in groups {
        if idx.len() >= GROUP_MIN {
            let sources: Vec<&str> = idx.iter().map(|&i| failed[i].source.as_str()).collect();
            out.push((
                idx[0],
                FailLine::Group {
                    folder: common_folder(&sources),
                    reason: why,
                    count: idx.len(),
                },
            ));
        } else {
            for i in idx {
                out.push((
                    i,
                    FailLine::File {
                        source: &failed[i].source,
                        reason: why,
                    },
                ));
            }
        }
    }
    out.sort_by_key(|(i, _)| *i);
    out.into_iter().map(|(_, l)| l).collect()
}

/// Deepest folder holding every one of `sources` (Windows or `/` paths).
fn common_folder(sources: &[&str]) -> String {
    let sep = if sources.iter().any(|s| s.contains('\\')) {
        '\\'
    } else {
        '/'
    };
    let split = |s: &str| -> Vec<String> {
        let mut parts: Vec<String> = s.split(['\\', '/']).map(str::to_owned).collect();
        parts.pop(); // the file name
        parts
    };
    let mut common = split(sources[0]);
    for s in &sources[1..] {
        let parts = split(s);
        let same = common
            .iter()
            .zip(&parts)
            .take_while(|(a, b)| a.eq_ignore_ascii_case(b))
            .count();
        common.truncate(same);
    }
    let mut folder = common.join(&sep.to_string());
    // `C:` → `C:\`, `/` (split as one empty part) → `/`; no common
    // folder at all stays empty.
    if folder.ends_with(':') || (folder.is_empty() && !common.is_empty()) {
        folder.push(sep);
    }
    folder
}

const ACCESS_DENIED: &str = "\"Access is denied\" (os error 5): a program that is still \
    running is usually holding these files. Close it, or sign that user out (Task Manager > \
    Users > Sign off), then resume the backup. Some antivirus programs also block reading \
    browser data. A backup from the Rebootly rescue stick avoids both.";
const IN_USE: &str = "\"Being used by another process\" (os error 32/33): the file was open \
    and no shadow copy could be made. Close the program and resume the backup.";

/// One hint per kind of error among the failed files.
pub fn hints(files: &[FileEntry]) -> Vec<&'static str> {
    let failed = || {
        files
            .iter()
            .filter(|f| f.status == FileStatus::Failed)
            .map(reason)
    };
    let mut out = Vec::new();
    if failed().any(|r| r.contains("(os error 5)")) {
        out.push(ACCESS_DENIED);
    }
    if failed().any(|r| r.contains("(os error 32)") || r.contains("(os error 33)")) {
        out.push(IN_USE);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failed(path: &str, source: &str, reason: &str) -> FileEntry {
        FileEntry {
            path: path.into(),
            source: source.into(),
            size: 1,
            mtime: None,
            attributes: None,
            sha256: None,
            status: FileStatus::Failed,
            reason: Some(reason.into()),
            snapshot: false,
            streams: vec![],
        }
    }

    const DENIED: &str = "Access is denied. (os error 5)";

    #[test]
    fn many_files_in_one_folder_make_one_line() {
        let edge = r"C:\Users\Amouni\AppData\Local\Microsoft\Edge\User Data";
        let mut files: Vec<FileEntry> = (0..7)
            .map(|i| {
                failed(
                    &format!("users/Amouni/packs/edge/Default/f{i}"),
                    &format!(r"{edge}\Default\Sub{}\f{i}", i % 3),
                    DENIED,
                )
            })
            .collect();
        files.insert(
            2,
            failed(
                "users/Amouni/folders/Documents/a.txt",
                r"C:\Users\Amouni\Documents\a.txt",
                "bad sector",
            ),
        );
        let mut ok = failed("users/Amouni/folders/Documents/b.txt", "b", "x");
        ok.status = FileStatus::Ok;
        files.push(ok);
        let l = lines(&files);
        assert_eq!(
            l,
            vec![
                FailLine::Group {
                    folder: format!(r"{edge}\Default"),
                    reason: DENIED,
                    count: 7
                },
                FailLine::File {
                    source: r"C:\Users\Amouni\Documents\a.txt",
                    reason: "bad sector"
                },
            ]
        );
        assert_eq!(hints(&files), vec![ACCESS_DENIED]);
    }

    #[test]
    fn few_files_or_different_reasons_stay_single() {
        let files: Vec<FileEntry> = (0..6)
            .map(|i| {
                let why = if i % 2 == 0 { DENIED } else { "other" };
                failed(
                    &format!("users/A/folders/Documents/{i}"),
                    &format!(r"C:\D\{i}"),
                    why,
                )
            })
            .collect();
        let l = lines(&files);
        assert_eq!(l.len(), 6);
        assert!(matches!(
            l[0],
            FailLine::File {
                source: r"C:\D\0",
                ..
            }
        ));
        assert!(matches!(
            l[5],
            FailLine::File {
                source: r"C:\D\5",
                ..
            }
        ));
    }

    #[test]
    fn common_folder_of_paths() {
        assert_eq!(common_folder(&[r"C:\a\b\x", r"c:\A\c\y"]), r"C:\a");
        assert_eq!(common_folder(&[r"C:\x", r"D:\y"]), "");
        assert_eq!(common_folder(&[r"C:\x", r"C:\y"]), r"C:\");
        assert_eq!(common_folder(&["/mnt/a/x", "/mnt/a/y"]), "/mnt/a");
        assert_eq!(row("users/A/packs/edge/Default/x"), "users/A/packs/edge");
        assert_eq!(row("system/hosts"), "system/hosts");
        assert_eq!(row("system/tasks/x"), "system/tasks");
    }

    #[test]
    fn in_use_hint() {
        let f = [failed(
            "system/x",
            "C:\\x",
            "The process cannot access the file because it is being used by another process. (os error 32)",
        )];
        assert_eq!(hints(&f), vec![IN_USE]);
        assert!(hints(&[failed("system/x", "x", "bad sector")]).is_empty());
    }
}
