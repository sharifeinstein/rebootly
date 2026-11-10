//! Choosing the source: an offline volume (`--windows-root`) or the running
//! system (`--live`, Windows only).

use std::path::PathBuf;

use rebootly_core::{OfflineVolume, Source};

/// Open the running system. Needs an elevated prompt.
pub fn live() -> anyhow::Result<Box<dyn Source>> {
    #[cfg(windows)]
    {
        Ok(Box::new(rebootly_win::LiveWindows::open()?))
    }
    #[cfg(not(windows))]
    {
        anyhow::bail!("--live works only on a running Windows; use --windows-root here")
    }
}

/// `--live` or the given volume root.
pub fn open(live_mode: bool, root: Option<&PathBuf>) -> anyhow::Result<Box<dyn Source>> {
    match (live_mode, root) {
        (true, _) => live(),
        (false, Some(r)) => Ok(Box::new(OfflineVolume::open(r)?)),
        (false, None) => anyhow::bail!("give --windows-root <PATH> or --live"),
    }
}

/// Keeps the PC awake while it lives (live Windows only).
pub struct Awake {
    #[cfg(windows)]
    _guard: Option<rebootly_win::SleepGuard>,
}

impl Awake {
    /// Keeps the PC awake (live Windows only). Front ends warn about
    /// battery themselves (`rebootly_win::on_battery`).
    pub fn new(live_mode: bool) -> Self {
        #[cfg(windows)]
        {
            Awake {
                _guard: live_mode.then(rebootly_win::SleepGuard::new),
            }
        }
        #[cfg(not(windows))]
        {
            let _ = live_mode;
            Awake {}
        }
    }
}
