//! Files that don't fit the new account, found in the VM tests: shortcuts
//! and binary app files with absolute paths of the old profile inside, and
//! files named after the old account's SID. Restoring them would leave
//! entries that point at a user who doesn't exist on the new PC.

use super::conflict::Category;
use super::map::FileAction;
use super::rewrite::MAX_REWRITE;
use super::target::Target;
use crate::backup::manifest::UserRecord;

pub fn is_shortcut(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.ends_with(".lnk") || n.ends_with(".url")
}

/// ASCII-case-insensitive search for `path` as single-byte text or UTF-16LE.
pub fn mentions_path(bytes: &[u8], path: &str) -> bool {
    let p = path.trim_end_matches('\\').as_bytes();
    if p.is_empty() {
        return false;
    }
    let narrow = bytes.windows(p.len()).any(|w| w.eq_ignore_ascii_case(p));
    let wide: Vec<u8> = p.iter().flat_map(|&c| [c, 0]).collect();
    narrow
        || bytes
            .windows(wide.len())
            .any(|w| w.iter().zip(&wide).all(|(a, b)| a.eq_ignore_ascii_case(b)))
}

fn leaf(win: &str) -> &str {
    win.rsplit('\\').next().unwrap_or(win)
}

/// Why `f` shouldn't be restored for this user, if it shouldn't.
/// `exists`: something is at the destination; `rewritten`: its old
/// profile paths were already rewritten.
pub fn unfit(
    f: &FileAction,
    exists: bool,
    rewritten: bool,
    old: &UserRecord,
    target: &Target,
) -> Option<String> {
    let name = leaf(&f.dst);
    let same_account = target
        .sid
        .as_deref()
        .is_some_and(|s| s.eq_ignore_ascii_case(&old.sid));
    if !old.sid.is_empty()
        && !same_account
        && name
            .to_ascii_uppercase()
            .contains(&old.sid.to_ascii_uppercase())
    {
        return Some("named after the old account; Windows makes its own for this user".into());
    }
    if exists && is_shortcut(name) {
        return Some(
            "a shortcut with this name is already here (it points to the right places on this PC)"
                .into(),
        );
    }
    let moved = !old.profile_path.eq_ignore_ascii_case(&target.profile);
    if moved && f.category == Category::Packs && !rewritten && f.size <= MAX_REWRITE {
        let bytes = std::fs::read(&f.src).ok()?;
        let text = std::str::from_utf8(&bytes).is_ok_and(|_| !bytes.contains(&0));
        if !text && mentions_path(&bytes, &old.profile_path) {
            return Some(format!(
                "has paths of the old profile ({}) inside, in a format that can't be rewritten",
                old.profile_path
            ));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_paths_in_narrow_and_wide_bytes() {
        let mut lnk = b"L\0\0\0junk".to_vec();
        lnk.extend(
            "c:\\users\\PICARD\\Desktop"
                .encode_utf16()
                .flat_map(u16::to_le_bytes),
        );
        assert!(mentions_path(&lnk, r"C:\Users\Picard"));
        assert!(mentions_path(
            b"\x01C:\\USERS\\picard\\x\0",
            r"C:\Users\Picard\"
        ));
        assert!(!mentions_path(&lnk, r"C:\Users\Riker"));
        assert!(is_shortcut("Desktop.LNK") && is_shortcut("Bing.url") && !is_shortcut("a.txt"));
    }
}
