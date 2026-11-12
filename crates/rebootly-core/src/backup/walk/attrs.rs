//! Windows file attributes of a source file, where the platform shows them.
//!
//! Windows: from the directory listing. Linux: the `system.ntfs_attrib_be`
//! extended attribute, which both ntfs3 and ntfs-3g provide, so online-only
//! OneDrive files are recognised offline too (ntfs3 would read them as
//! zeros). Other filesystems: none.

use std::path::Path;

const FILE_ATTRIBUTE_OFFLINE: u32 = 0x1000;
const FILE_ATTRIBUTE_RECALL_ON_OPEN: u32 = 0x4_0000;
const FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS: u32 = 0x40_0000;

/// An online-only cloud file (OneDrive Files On-Demand): opening it would
/// download it. Files kept on the device ("always keep") have none of these.
pub fn is_cloud_placeholder(attrs: u32) -> bool {
    attrs
        & (FILE_ATTRIBUTE_OFFLINE
            | FILE_ATTRIBUTE_RECALL_ON_OPEN
            | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS)
        != 0
}

#[cfg(windows)]
pub fn attributes(_path: &Path, m: &std::fs::Metadata) -> Option<u32> {
    use std::os::windows::fs::MetadataExt;
    Some(m.file_attributes())
}

#[cfg(target_os = "linux")]
pub fn attributes(path: &Path, _m: &std::fs::Metadata) -> Option<u32> {
    let v = xattr::get(path, "system.ntfs_attrib_be").ok()??;
    Some(u32::from_be_bytes(v.as_slice().try_into().ok()?))
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn attributes(_path: &Path, _m: &std::fs::Metadata) -> Option<u32> {
    None
}

/// ntfs-3g shows a file whose reparse tag it has no plugin for (OneDrive
/// files among them) as a symlink to this text; its data can't be read.
pub fn unsupported_reparse_tag(link_target: &str) -> Option<&str> {
    link_target.strip_prefix("unsupported reparse tag ")
}
