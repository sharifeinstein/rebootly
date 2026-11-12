//! The Windows licence of an install, read from its SOFTWARE hive (works
//! the same live, in WinPE and from Linux): the key in `DigitalProductId`,
//! the channel in `DigitalProductId4`, and what the user can do with them.
//! The key itself goes only into the encrypted bundle.

pub mod key;

use crate::backup::manifest::ManualStep;
use crate::hive::{Hive, Result as HiveResult};
use crate::install::CURRENT_VERSION;
pub use key::ProductKey;

const SPP: &str = r"Microsoft\Windows NT\CurrentVersion\SoftwareProtectionPlatform";
/// Where the full key goes inside `sensitive.age`.
pub const BUNDLE_PATH: &str = "system/licence/windows-key.txt";

#[derive(Debug)]
pub struct WindowsLicence {
    pub key: Option<ProductKey>,
    /// `Retail`, `OEM:DM`, `OEM:NONSLP`, `Volume:MAK`, `Volume:GVLK`, …
    pub channel: Option<String>,
    pub edition: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A generic key: the licence is a digital licence, nothing to copy.
    Digital,
    Retail,
    /// The PC maker's key from the firmware (`OEM:DM`).
    Firmware,
    /// Another OEM key (sticker, `OEM:NONSLP`, `OEM:COA`).
    Oem,
    /// An organisation's multiple-activation key.
    Mak,
    /// Activated by an organisation's KMS server.
    Kms,
    /// A key whose channel isn't recorded.
    Unknown,
}

impl WindowsLicence {
    pub fn kind(&self) -> Kind {
        let ch = self.channel.as_deref().unwrap_or("").to_ascii_uppercase();
        let key = self.key.as_ref();
        if key.is_some_and(ProductKey::is_kms) || ch.starts_with("VOLUME:GVLK") {
            Kind::Kms
        } else if key.is_none() || key.is_some_and(ProductKey::is_generic) {
            Kind::Digital
        } else if ch.starts_with("VOLUME") {
            Kind::Mak
        } else if ch == "OEM:DM" {
            Kind::Firmware
        } else if ch.starts_with("OEM") {
            Kind::Oem
        } else if ch == "RETAIL" {
            Kind::Retail
        } else {
            Kind::Unknown
        }
    }

    /// The key is worth keeping (in the encrypted bundle).
    pub fn keep_key(&self) -> bool {
        !matches!(self.kind(), Kind::Digital | Kind::Kms)
    }

    /// The text kept in the bundle, `None` when there is nothing to keep.
    pub fn bundle_text(&self, machine: &str) -> Option<String> {
        let key = self.key.as_ref().filter(|_| self.keep_key())?;
        Some(format!(
            "Windows product key of {machine}\r\nEdition: {}\r\nChannel: {}\r\nKey: {}\r\n",
            self.edition.as_deref().unwrap_or("?"),
            self.channel.as_deref().unwrap_or("?"),
            key.expose()
        ))
    }

    /// One line for `inspect`, e.g. `retail key ending 3V66T (Professional)`.
    pub fn summary(&self) -> String {
        let title = self.manual_step(false).title;
        let what = title.trim_start_matches("Windows licence: ");
        let tail = match (&self.key, self.kind()) {
            (Some(k), kind) if !matches!(kind, Kind::Digital | Kind::Kms) => {
                format!(", key ending {}", k.last5())
            }
            _ => String::new(),
        };
        format!(
            "{what}{tail} ({}, channel {})",
            self.edition.as_deref().unwrap_or("edition ?"),
            self.channel.as_deref().unwrap_or("?")
        )
    }

    /// The checklist step (never holds the key, only its last group).
    pub fn manual_step(&self, bundle: bool) -> ManualStep {
        let edition = self.edition.as_deref().unwrap_or("the same edition");
        let tail = self.key.as_ref().map_or("?????", ProductKey::last5);
        let kept = if !self.keep_key() {
            ""
        } else if bundle {
            " The full key is in sensitive.age (system/licence/windows-key.txt)."
        } else {
            " The full key is only kept in the encrypted bundle: choose Full (or --sensitive) \
             to keep it."
        };
        let move_it = format!(
            "Activation needs the same edition on the new PC ({edition}): Settings > System > \
             Activation."
        );
        let (title, detail) = match self.kind() {
            Kind::Digital => (
                "Windows licence: digital licence, no key to copy",
                format!(
                    "Windows here uses a generic key, so the licence is held by Microsoft. A new \
                     PC that came with Windows needs nothing. To move a licence you bought, sign \
                     in with the Microsoft account linked to it (Activation shows whether it is \
                     linked) and run Activation > Troubleshoot > \"I changed hardware on this \
                     device recently\". A licence that came with the old PC stays with it. \
                     {move_it}"
                ),
            ),
            Kind::Retail => (
                "Windows licence: retail key",
                format!(
                    "Key ending {tail}. A retail key can move to another PC (one at a time): \
                     Activation > Change product key. {move_it}{kept}"
                ),
            ),
            Kind::Firmware => (
                "Windows licence: the old PC's own key",
                format!(
                    "Key ending {tail} is stored in the old PC's firmware and belongs to that PC: \
                     it won't activate a different one. Keep it for reinstalling the old PC.{kept}"
                ),
            ),
            Kind::Oem => (
                "Windows licence: key that came with the old PC",
                format!(
                    "Key ending {tail} (OEM) is tied to the old PC and won't activate a \
                     different one.{kept}"
                ),
            ),
            Kind::Mak => (
                "Windows licence: organisation's volume key",
                format!(
                    "Key ending {tail} (volume licence, MAK) belongs to an organisation: ask its \
                     IT before using it on another PC.{kept}"
                ),
            ),
            Kind::Kms => (
                "Windows licence: activated by an organisation",
                "This Windows is activated by an organisation's KMS server: there is no key to \
                 copy. Its IT activates the new PC."
                    .to_owned(),
            ),
            Kind::Unknown => (
                "Windows licence: product key",
                format!(
                    "Key ending {tail} (type not recorded). If it is a retail key it can move to \
                     the new PC: Activation > Change product key. {move_it}{kept}"
                ),
            ),
        };
        ManualStep {
            title: title.into(),
            detail,
        }
    }
}

/// Read the licence from a SOFTWARE hive. `None` when there is no key
/// information at all.
pub fn read(software: &Hive) -> HiveResult<Option<WindowsLicence>> {
    let Some(cv) = software.open_key(CURRENT_VERSION)? else {
        return Ok(None);
    };
    let mut key = cv
        .value("DigitalProductId")?
        .and_then(|v| key::decode(&v.data));
    if key.is_none() {
        key = software
            .open_key(SPP)?
            .map(|k| k.string("BackupProductKeyDefault"))
            .transpose()?
            .flatten()
            .and_then(|s| key::parse(&s));
    }
    let channel = cv
        .value("DigitalProductId4")?
        .and_then(|v| channel_of(&v.data));
    if key.is_none() && channel.is_none() {
        return Ok(None);
    }
    Ok(Some(WindowsLicence {
        key,
        channel,
        edition: cv.string("EditionID")?,
    }))
}

/// The licence channel in `DigitalProductId4` (UTF-16 strings in a fixed
/// layout; found by its value rather than by offset).
fn channel_of(data: &[u8]) -> Option<String> {
    let units: Vec<u16> = data
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16_lossy(&units)
        .split(|c: char| !c.is_ascii_graphic())
        .find(|s| {
            let u = s.to_ascii_uppercase();
            u == "RETAIL"
                || ((u.starts_with("OEM:") || u.starts_with("VOLUME:"))
                    && u.len() <= 16
                    && u[u.find(':').unwrap_or(0) + 1..]
                        .chars()
                        .all(|c| c.is_ascii_alphabetic()))
        })
        .map(str::to_owned)
}

#[cfg(test)]
mod tests;
