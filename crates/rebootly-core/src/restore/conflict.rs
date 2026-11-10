//! Conflict policy (SPEC §5): what to do when a file is already there.
//! Identical files (same size and SHA-256) are always left alone.

use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Where a restored file comes from; each has its own policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    /// Tier 1 user folders.
    Folders,
    /// Custom, Public and drive-root folders.
    Custom,
    /// App data from packs.
    Packs,
    /// System files (hosts, all-users VPN phonebook).
    System,
}

impl Category {
    pub const ALL: [Category; 4] = [
        Category::Folders,
        Category::Custom,
        Category::Packs,
        Category::System,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Category::Folders => "folders",
            Category::Custom => "custom",
            Category::Packs => "packs",
            Category::System => "system",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Policy {
    Skip,
    /// Replace it (the old file is kept for undo).
    Overwrite,
    /// Replace it only if the backup's copy is newer.
    OverwriteIfNewer,
    /// Write `name (migrated).ext` beside it.
    KeepBoth,
    Ask,
}

impl Policy {
    pub fn as_str(self) -> &'static str {
        match self {
            Policy::Skip => "skip",
            Policy::Overwrite => "overwrite",
            Policy::OverwriteIfNewer => "overwrite-if-newer",
            Policy::KeepBoth => "keep-both",
            Policy::Ask => "ask",
        }
    }
}

impl FromStr for Policy {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        Ok(match s.trim().to_ascii_lowercase().as_str() {
            "skip" => Policy::Skip,
            "overwrite" => Policy::Overwrite,
            "overwrite-if-newer" | "newer" => Policy::OverwriteIfNewer,
            "keep-both" | "both" => Policy::KeepBoth,
            "ask" => Policy::Ask,
            _ => {
                return Err(format!(
                    "unknown conflict policy `{s}` (skip, overwrite, overwrite-if-newer, keep-both, ask)"
                ));
            }
        })
    }
}

/// One policy per category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Policies {
    pub folders: Policy,
    pub custom: Policy,
    pub packs: Policy,
    pub system: Policy,
}

impl Default for Policies {
    /// Your files are never replaced by default; app settings are (the
    /// freshly installed app's defaults are what the old settings replace,
    /// and they're kept for undo).
    fn default() -> Self {
        Policies {
            folders: Policy::KeepBoth,
            custom: Policy::KeepBoth,
            packs: Policy::Overwrite,
            system: Policy::Overwrite,
        }
    }
}

impl Policies {
    pub fn all(p: Policy) -> Self {
        Policies {
            folders: p,
            custom: p,
            packs: p,
            system: p,
        }
    }

    pub fn get(&self, c: Category) -> Policy {
        match c {
            Category::Folders => self.folders,
            Category::Custom => self.custom,
            Category::Packs => self.packs,
            Category::System => self.system,
        }
    }

    pub fn set(&mut self, c: Category, p: Policy) {
        match c {
            Category::Folders => self.folders = p,
            Category::Custom => self.custom = p,
            Category::Packs => self.packs = p,
            Category::System => self.system = p,
        }
    }

    /// Apply `folders=skip,packs=newer` on top of these.
    pub fn apply(&mut self, spec: &str) -> Result<(), String> {
        for part in spec.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            let (cat, pol) = part
                .split_once('=')
                .ok_or_else(|| format!("expected CATEGORY=POLICY, got `{part}`"))?;
            let cat = Category::ALL
                .into_iter()
                .find(|c| c.as_str().eq_ignore_ascii_case(cat.trim()))
                .ok_or_else(|| {
                    format!("unknown category `{cat}` (folders, custom, packs, system)")
                })?;
            self.set(cat, pol.parse()?);
        }
        Ok(())
    }
}

/// `report.txt` → `report (migrated).txt`, then `report (migrated 2).txt`…
/// A leading dot isn't an extension (`.gitconfig (migrated)`).
pub fn keep_both_name(win_path: &str, n: u32) -> String {
    let (dir, name) = match win_path.rfind('\\') {
        Some(i) => (&win_path[..=i], &win_path[i + 1..]),
        None => ("", win_path),
    };
    let tag = if n <= 1 {
        " (migrated)".to_owned()
    } else {
        format!(" (migrated {n})")
    };
    match name.rfind('.').filter(|&i| i > 0) {
        Some(i) => format!("{dir}{}{tag}{}", &name[..i], &name[i..]),
        None => format!("{dir}{name}{tag}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_parsing() {
        assert_eq!(
            keep_both_name(r"C:\U\report.txt", 1),
            r"C:\U\report (migrated).txt"
        );
        assert_eq!(
            keep_both_name(r"C:\U\a.tar.gz", 2),
            r"C:\U\a.tar (migrated 2).gz"
        );
        assert_eq!(
            keep_both_name(r"C:\U\.gitconfig", 1),
            r"C:\U\.gitconfig (migrated)"
        );
        assert_eq!(
            keep_both_name(r"C:\U\Makefile", 1),
            r"C:\U\Makefile (migrated)"
        );
        let mut p = Policies::default();
        p.apply("folders=skip, packs=newer").unwrap();
        assert_eq!(p.folders, Policy::Skip);
        assert_eq!(p.packs, Policy::OverwriteIfNewer);
        assert_eq!(p.custom, Policy::KeepBoth);
        assert!(p.apply("music=skip").is_err());
        assert!(p.apply("packs=sometimes").is_err());
    }
}
