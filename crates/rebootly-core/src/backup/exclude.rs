//! Always-on exclusions (SPEC §3.6).
//!
//! Rules match case-insensitively on path components relative to the user's
//! profile root (or to the folder root for folders outside a profile).
//! AppData-specific rules only fire under `AppData\`, so a user's own
//! `Documents\Cache` folder is still copied.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    /// pagefile.sys, hiberfil.sys, swapfile.sys
    SystemFile,
    RecycleBin,
    SystemVolumeInformation,
    Temp,
    /// Outlook `.ost` caches (rebuilt from the server).
    OutlookCache,
    /// Thumbs.db
    ThumbnailCache,
    /// INetCache, CrashDumps, WER
    WindowsCache,
    BrowserCache,
    /// Store apps' `Packages\*\AC` and `Packages\*\TempState`
    StoreAppTemp,
    /// NTUSER.DAT*, UsrClass.dat*: copied raw to `system/hives/` instead.
    RegistryHive,
    /// Browser password/cookie stores and Credential Manager / DPAPI data.
    /// Never copied: they can't be decrypted elsewhere and copying them
    /// looks like infostealer behaviour.
    Secrets,
    /// node_modules, .git/objects (optional)
    DevArtifacts,
    /// OneDrive (or other cloud) online-only placeholders: reading them would
    /// download them, so the default policy is to skip (SPEC §3.5).
    CloudPlaceholder,
}

impl Category {
    pub fn describe(self) -> &'static str {
        match self {
            Category::SystemFile => "page/hibernation/swap files",
            Category::RecycleBin => "Recycle Bin",
            Category::SystemVolumeInformation => "System Volume Information",
            Category::Temp => "temporary files",
            Category::OutlookCache => "Outlook .ost caches",
            Category::ThumbnailCache => "Thumbs.db",
            Category::WindowsCache => "Windows caches and crash dumps",
            Category::BrowserCache => "browser caches",
            Category::StoreAppTemp => "Store app temp data",
            Category::RegistryHive => "registry hives (copied raw to system/hives)",
            Category::Secrets => "browser passwords/cookies and credential stores (never copied)",
            Category::DevArtifacts => "node_modules and .git/objects",
            Category::CloudPlaceholder => {
                "online-only OneDrive files (not downloaded; still in the cloud)"
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ExcludeOptions {
    /// Also skip `node_modules` and `.git/objects`.
    pub dev_artifacts: bool,
}

fn eq(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

fn any(name: &str, set: &[&str]) -> bool {
    set.iter().any(|s| eq(name, s))
}

/// `comps` ends with `suffix` (case-insensitive).
fn ends_with(comps: &[String], suffix: &[&str]) -> bool {
    comps.len() >= suffix.len()
        && comps[comps.len() - suffix.len()..]
            .iter()
            .zip(suffix)
            .all(|(a, b)| eq(a, b))
}

fn starts_with(comps: &[String], prefix: &[&str]) -> bool {
    comps.len() >= prefix.len() && comps.iter().zip(prefix).all(|(a, b)| eq(a, b))
}

const BROWSER_CACHE_DIRS: &[&str] = &[
    "Cache",
    "Code Cache",
    "GPUCache",
    "ShaderCache",
    "GrShaderCache",
    "cache2",
];
const SECRET_FILES: &[&str] = &[
    "Login Data",
    "Login Data For Account",
    "Cookies",
    "Local State",
    "logins.json",
    "key3.db",
    "key4.db",
    "cookies.sqlite",
    "signons.sqlite",
];

/// Classify one entry. `comps` is the full relative path *including* the
/// entry's own name as the last element. `profile_relative` says whether
/// `comps` starts at the profile root (enables AppData rules).
pub fn classify(
    comps: &[String],
    is_dir: bool,
    profile_relative: bool,
    opts: ExcludeOptions,
) -> Option<Category> {
    let name = comps.last()?.as_str();
    let up = name.to_ascii_uppercase();

    // Anywhere.
    if !is_dir && any(name, &["pagefile.sys", "hiberfil.sys", "swapfile.sys"]) {
        return Some(Category::SystemFile);
    }
    if is_dir && eq(name, "$Recycle.Bin") {
        return Some(Category::RecycleBin);
    }
    if is_dir && eq(name, "System Volume Information") {
        return Some(Category::SystemVolumeInformation);
    }
    if !is_dir && up.ends_with(".OST") {
        return Some(Category::OutlookCache);
    }
    if !is_dir && eq(name, "Thumbs.db") {
        return Some(Category::ThumbnailCache);
    }
    if !is_dir && (up.starts_with("NTUSER.DAT") || up.starts_with("USRCLASS.DAT")) {
        return Some(Category::RegistryHive);
    }
    if opts.dev_artifacts
        && is_dir
        && (eq(name, "node_modules") || ends_with(comps, &[".git", "objects"]))
    {
        return Some(Category::DevArtifacts);
    }

    // AppData only.
    let in_appdata = profile_relative && comps.len() > 1 && eq(&comps[0], "AppData");
    if !in_appdata {
        return None;
    }
    if is_dir && starts_with(comps, &["AppData", "Local", "Temp"]) && comps.len() == 3 {
        return Some(Category::Temp);
    }
    if is_dir {
        for secret_dir in [
            &["AppData", "Roaming", "Microsoft", "Credentials"][..],
            &["AppData", "Local", "Microsoft", "Credentials"],
            &["AppData", "Roaming", "Microsoft", "Vault"],
            &["AppData", "Local", "Microsoft", "Vault"],
            &["AppData", "Roaming", "Microsoft", "Protect"],
        ] {
            if comps.len() == secret_dir.len() && starts_with(comps, secret_dir) {
                return Some(Category::Secrets);
            }
        }
        if any(name, &["INetCache", "CrashDumps"])
            || ends_with(comps, &["Microsoft", "Windows", "WER"])
        {
            return Some(Category::WindowsCache);
        }
        if any(name, BROWSER_CACHE_DIRS) || ends_with(comps, &["Service Worker", "CacheStorage"]) {
            return Some(Category::BrowserCache);
        }
        // AppData\Local\Packages\<pkg>\AC | TempState
        if comps.len() == 5
            && starts_with(comps, &["AppData", "Local", "Packages"])
            && any(name, &["AC", "TempState"])
        {
            return Some(Category::StoreAppTemp);
        }
    } else if any(name, SECRET_FILES) {
        return Some(Category::Secrets);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(path: &str, is_dir: bool) -> Option<Category> {
        let comps: Vec<String> = path.split('\\').map(str::to_owned).collect();
        classify(&comps, is_dir, true, ExcludeOptions::default())
    }

    #[test]
    fn generic_rules() {
        assert_eq!(
            c(r"Documents\Thumbs.DB", false),
            Some(Category::ThumbnailCache)
        );
        assert_eq!(c(r"Documents\desktop.ini", false), None);
        assert_eq!(c(r"NTUSER.DAT.LOG1", false), Some(Category::RegistryHive));
        assert_eq!(
            c(r"Documents\mail.ost", false),
            Some(Category::OutlookCache)
        );
        assert_eq!(c(r"Documents\mail.pst", false), None);
        assert_eq!(c(r"$RECYCLE.BIN", true), Some(Category::RecycleBin));
    }

    #[test]
    fn appdata_rules_are_scoped() {
        assert_eq!(c(r"Documents\Cache", true), None);
        assert_eq!(c(r"Documents\Cookies", false), None);
        let chrome = r"AppData\Local\Google\Chrome\User Data\Default";
        assert_eq!(
            c(&format!(r"{chrome}\Cache"), true),
            Some(Category::BrowserCache)
        );
        assert_eq!(
            c(&format!(r"{chrome}\Login Data"), false),
            Some(Category::Secrets)
        );
        assert_eq!(
            c(&format!(r"{chrome}\Network\Cookies"), false),
            Some(Category::Secrets)
        );
        assert_eq!(
            c(&format!(r"{chrome}\Service Worker\CacheStorage"), true),
            Some(Category::BrowserCache)
        );
        assert_eq!(c(r"AppData\Local\Temp", true), Some(Category::Temp));
        assert_eq!(c(r"AppData\Local\Temp\x\Temp", true), None);
        assert_eq!(
            c(r"AppData\Roaming\Microsoft\Protect", true),
            Some(Category::Secrets)
        );
        assert_eq!(
            c(
                r"AppData\Local\Packages\Microsoft.App_8wekyb3d8bbwe\TempState",
                true
            ),
            Some(Category::StoreAppTemp)
        );
        assert_eq!(c(r"AppData\Local\Packages\X\LocalState", true), None);
        assert_eq!(
            c(r"AppData\Local\Microsoft\Windows\WER", true),
            Some(Category::WindowsCache)
        );
    }

    #[test]
    fn dev_artifacts_optional() {
        let comps: Vec<String> = ["src", "app", "node_modules"].map(String::from).to_vec();
        assert_eq!(
            classify(&comps, true, true, ExcludeOptions::default()),
            None
        );
        let on = ExcludeOptions {
            dev_artifacts: true,
        };
        assert_eq!(
            classify(&comps, true, true, on),
            Some(Category::DevArtifacts)
        );
        let git: Vec<String> = ["r", ".git", "objects"].map(String::from).to_vec();
        assert_eq!(
            classify(&git, true, false, on),
            Some(Category::DevArtifacts)
        );
    }

    #[test]
    fn appdata_rules_need_profile_context() {
        let comps: Vec<String> = ["AppData", "Local", "Temp"].map(String::from).to_vec();
        assert_eq!(
            classify(&comps, true, false, ExcludeOptions::default()),
            None
        );
    }
}
