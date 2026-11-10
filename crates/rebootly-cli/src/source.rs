//! Choosing the source (`--windows-root` / `--live`) and keeping the PC
//! awake: the shared machine layer, plus the CLI's battery warning.

pub use rebootly_host::source::open;

/// Keeps the PC awake for the job and warns on battery (live Windows).
pub struct Awake(#[allow(dead_code)] rebootly_host::source::Awake);

impl Awake {
    pub fn new(live_mode: bool) -> Self {
        #[cfg(windows)]
        if live_mode && rebootly_win::on_battery() {
            eprintln!("warning: running on battery; plug the laptop in for a long backup");
        }
        Awake(rebootly_host::source::Awake::new(live_mode))
    }
}
