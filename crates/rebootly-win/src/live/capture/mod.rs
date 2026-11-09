//! Live Tier 4 captures by command (SPEC §4 Tier 4). Inventory (always):
//! winget export, printers, power plan, default apps, driver list. Secrets
//! (only when the encrypted bundle is available; they go nowhere else):
//! Wi-Fi with keys, user certificates with private keys. Outputs land in a
//! private temp folder that is removed on drop.

mod inventory;
mod secrets;

use std::path::PathBuf;
use std::sync::Mutex;

use rebootly_core::{Capture, Captures};

#[derive(Debug)]
pub struct Capturer {
    dir: Mutex<Option<PathBuf>>,
}

impl Capturer {
    pub fn new() -> Self {
        Capturer {
            dir: Mutex::new(None),
        }
    }

    fn fresh_dir(&self) -> std::io::Result<PathBuf> {
        let mut g = self.dir.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(old) = g.take() {
            let _ = std::fs::remove_dir_all(old);
        }
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        let d =
            std::env::temp_dir().join(format!("rebootly-capture-{}-{stamp}", std::process::id()));
        std::fs::create_dir(&d)?;
        *g = Some(d.clone());
        Ok(d)
    }

    pub fn run(&self, sensitive: bool) -> Captures {
        let mut out = Captures::default();
        let dir = match self.fresh_dir() {
            Ok(d) => d,
            Err(e) => {
                out.warnings
                    .push(format!("no temp folder for captures: {e}"));
                return out;
            }
        };
        inventory::winget(&dir, &mut out);
        inventory::printers(&dir, &mut out);
        inventory::power_plan(&dir, &mut out);
        inventory::default_apps(&dir, &mut out);
        inventory::drivers(&dir, &mut out);
        if sensitive {
            secrets::wifi(&dir, &mut out);
            secrets::certificates(&dir, &mut out);
        }
        out
    }
}

impl Default for Capturer {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Capturer {
    fn drop(&mut self) {
        if let Some(d) = self.dir.get_mut().ok().and_then(|g| g.take()) {
            let _ = std::fs::remove_dir_all(d);
        }
    }
}

pub(super) fn push(out: &mut Captures, dest: &str, file: PathBuf, origin: &str, sensitive: bool) {
    out.files.push(Capture {
        dest: dest.into(),
        file,
        origin: origin.into(),
        sensitive,
    });
}
