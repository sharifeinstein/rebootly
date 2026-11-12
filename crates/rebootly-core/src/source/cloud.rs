//! Reading OneDrive files whose data is on the disk when OneDrive's filter
//! driver (`cldflt.sys`) isn't there: WinPE, and VSS shadow copies.
//!
//! Every file in a OneDrive folder carries a cloud reparse point, also when
//! it is fully downloaded. Without the filter Windows can't handle the tag,
//! and every open fails with `ERROR_CANT_ACCESS_FILE` (1920). Opening the
//! reparse point itself (`FILE_FLAG_OPEN_REPARSE_POINT`) skips the filter and
//! reads the file's own data stream, which is complete for a downloaded file.
//! Online-only and partly downloaded files are refused: their data isn't on
//! the disk and the stream would read as zeros. Sparse files are refused too
//! (also Data Deduplication's, whose data lives in a chunk store).

use std::fs::File;
use std::io;
use std::path::Path;

use crate::backup::walk::is_cloud_placeholder;

/// `ERROR_CANT_ACCESS_FILE`: no filter handles the file's reparse tag.
pub const ERROR_CANT_ACCESS_FILE: i32 = 1920;
/// `FILE_FLAG_OPEN_REPARSE_POINT`.
pub const OPEN_REPARSE_POINT: u32 = 0x0020_0000;
const FILE_ATTRIBUTE_SPARSE_FILE: u32 = 0x200;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

/// A reparse-point file whose whole data is in its own stream.
pub fn local_data_complete(attrs: u32) -> bool {
    attrs & FILE_ATTRIBUTE_REPARSE_POINT != 0
        && attrs & FILE_ATTRIBUTE_SPARSE_FILE == 0
        && !is_cloud_placeholder(attrs)
}

/// `open(extra_flags)` once with no extra flags; if Windows can't handle the
/// file's reparse tag and `attrs()` shows the data is on the disk, once more
/// opening the reparse point itself.
pub fn open_or_bypass(
    open: impl Fn(u32) -> io::Result<File>,
    attrs: impl FnOnce() -> io::Result<u32>,
) -> io::Result<File> {
    match open(0) {
        Err(e) if e.raw_os_error() == Some(ERROR_CANT_ACCESS_FILE) => match attrs() {
            Ok(a) if local_data_complete(a) => open(OPEN_REPARSE_POINT),
            Ok(_) => Err(io::Error::new(
                e.kind(),
                format!("{e}; a cloud file whose data isn't on this disk (online-only)"),
            )),
            Err(_) => Err(e),
        },
        r => r,
    }
}

/// Attributes of `path` itself (not following a reparse point).
#[cfg(windows)]
pub fn own_attributes(path: &Path) -> io::Result<u32> {
    use std::os::windows::fs::MetadataExt;
    Ok(std::fs::symlink_metadata(path)?.file_attributes())
}
#[cfg(not(windows))]
pub fn own_attributes(_path: &Path) -> io::Result<u32> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}

/// Read-only open as offline access does it, with the cloud-file bypass.
pub fn open_read_only(path: &Path) -> io::Result<File> {
    open_or_bypass(
        |extra| {
            let mut o = std::fs::OpenOptions::new();
            o.read(true);
            #[cfg(windows)]
            {
                use std::os::windows::fs::OpenOptionsExt;
                o.custom_flags(extra);
            }
            #[cfg(not(windows))]
            let _ = extra;
            o.open(path)
        },
        || own_attributes(path),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    const DOWNLOADED: u32 = 0x420; // REPARSE_POINT | ARCHIVE, as seen in WinPE
    const ONLINE_ONLY: u32 = 0x0040_0620; // + RECALL_ON_DATA_ACCESS | SPARSE

    #[test]
    fn which_files_may_be_bypassed() {
        assert!(local_data_complete(DOWNLOADED));
        assert!(!local_data_complete(ONLINE_ONLY));
        assert!(!local_data_complete(0x620)); // sparse (dedup)
        assert!(!local_data_complete(0x1420)); // OFFLINE
        assert!(!local_data_complete(0x20)); // no reparse point at all
    }

    /// Fails like WinPE without OneDrive's filter unless the reparse point
    /// itself is opened; records the flags of each try.
    fn fake(tries: &RefCell<Vec<u32>>, file: &Path) -> impl Fn(u32) -> io::Result<File> {
        move |extra| {
            tries.borrow_mut().push(extra);
            if extra & OPEN_REPARSE_POINT == 0 {
                Err(io::Error::from_raw_os_error(ERROR_CANT_ACCESS_FILE))
            } else {
                File::open(file)
            }
        }
    }

    #[test]
    fn downloaded_cloud_file_is_reopened_as_reparse_point() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let tries = RefCell::new(Vec::new());
        open_or_bypass(fake(&tries, tmp.path()), || Ok(DOWNLOADED)).unwrap();
        assert_eq!(*tries.borrow(), vec![0, OPEN_REPARSE_POINT]);
    }

    #[test]
    fn online_only_file_is_not_read_as_zeros() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let tries = RefCell::new(Vec::new());
        let e = open_or_bypass(fake(&tries, tmp.path()), || Ok(ONLINE_ONLY)).unwrap_err();
        assert_eq!(e.raw_os_error(), None);
        assert!(e.to_string().contains("online-only"), "{e}");
        assert_eq!(*tries.borrow(), vec![0]);
        // Attributes unknown: the original error, no second try.
        let tries = RefCell::new(Vec::new());
        let e = open_or_bypass(fake(&tries, tmp.path()), || {
            Err(io::Error::from(io::ErrorKind::Unsupported))
        })
        .unwrap_err();
        assert_eq!(e.raw_os_error(), Some(ERROR_CANT_ACCESS_FILE));
        assert_eq!(*tries.borrow(), vec![0]);
    }

    #[test]
    fn other_errors_and_successes_pass_through() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let ok = open_or_bypass(|_| File::open(tmp.path()), || unreachable!());
        assert!(ok.is_ok());
        let e = open_or_bypass(
            |_| Err(io::Error::from(io::ErrorKind::PermissionDenied)),
            || unreachable!(),
        )
        .unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::PermissionDenied);
    }
}
