//! Backup flow: source → users → Quick/Full/Custom → target → pre-flight →
//! progress → done. In WinPE/Linux the same flow is the Rescue flow.

pub mod custom;
pub mod done;
mod key;
pub mod mode;
pub mod preflight;
pub mod progress;
pub mod source;
pub mod target;
pub mod users;

pub use rebootly_host::session::{Account, BackupCtx, Shared};
