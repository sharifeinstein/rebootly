//! What the restore needs from the platform. The Windows implementation is
//! in rebootly-win; [`SimHost`] stands in for simulation and dry runs.

use std::path::{Path, PathBuf};

/// One system command. Each maps to one Windows tool call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SysOp {
    /// `winget import` (long-running; output goes to the console).
    WingetImport(PathBuf),
    /// `reg export <key> <out>`; `No` when the key doesn't exist.
    RegExport {
        key: String,
        out: PathBuf,
    },
    RegImport(PathBuf),
    RegDelete(String),
    WifiExists(String),
    WifiAdd(PathBuf),
    WifiDelete(String),
    /// GUID of the active power scheme.
    PowerActive,
    /// `powercfg /import`; returns the new scheme's GUID.
    PowerImport(PathBuf),
    PowerSetActive(String),
    PowerDelete(String),
    /// Certificate in `Cert:\CurrentUser\My` by thumbprint.
    CertExists(String),
    /// PFX (password read from `password` by the tool, never on a command
    /// line) or CER into `Cert:\CurrentUser\My`.
    CertImport {
        file: PathBuf,
        password: Option<PathBuf>,
    },
    CertDelete(String),
    TaskExists(String),
    TaskCreate {
        name: String,
        xml: PathBuf,
    },
    TaskDelete(String),
    RestartExplorer,
}

impl SysOp {
    /// Plain-language description for plans and reports.
    pub fn describe(&self) -> String {
        let f = |p: &PathBuf| {
            p.file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
        };
        match self {
            SysOp::WingetImport(p) => format!("winget import {}", f(p)),
            SysOp::RegExport { key, .. } => format!("save {key} for undo"),
            SysOp::RegImport(p) => format!("reg import {}", f(p)),
            SysOp::RegDelete(k) => format!("delete {k}"),
            SysOp::WifiExists(n) => format!("check Wi-Fi profile {n}"),
            SysOp::WifiAdd(p) => format!("netsh wlan add profile {}", f(p)),
            SysOp::WifiDelete(n) => format!("netsh wlan delete profile {n}"),
            SysOp::PowerActive => "powercfg /getactivescheme".into(),
            SysOp::PowerImport(p) => format!("powercfg /import {}", f(p)),
            SysOp::PowerSetActive(g) => format!("powercfg /setactive {g}"),
            SysOp::PowerDelete(g) => format!("powercfg /delete {g}"),
            SysOp::CertExists(t) => format!("check certificate {t}"),
            SysOp::CertImport { file, .. } => format!("import certificate {}", f(file)),
            SysOp::CertDelete(t) => format!("remove certificate {t}"),
            SysOp::TaskExists(n) => format!("check task {n}"),
            SysOp::TaskCreate { name, .. } => format!("schtasks /create {name}"),
            SysOp::TaskDelete(n) => format!("schtasks /delete {n}"),
            SysOp::RestartExplorer => "restart Explorer".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SysOk {
    Done,
    Yes,
    No,
    Value(String),
}

pub trait RestoreHost {
    /// Run one system command. `Err` is a plain-language reason.
    fn sys(&self, op: &SysOp) -> Result<SysOk, String>;
    /// Image names of running processes (`chrome.exe`), for `requires_closed`.
    fn running(&self) -> Vec<String>;
    /// Write `from` as the alternate data stream `name` of `file`.
    fn write_stream(&self, file: &Path, name: &str, from: &Path) -> std::io::Result<()>;
    /// Add file attributes (hidden, system) to a restored file.
    fn set_attributes(&self, _file: &Path, _attrs: u32) -> std::io::Result<()> {
        Ok(())
    }
    /// A simulation: files are written under a folder, system steps are
    /// only listed.
    fn simulated(&self) -> bool {
        false
    }
}

/// For `--simulate-root` and for previews off Windows: no system commands,
/// no streams, nothing running.
#[derive(Debug, Default)]
pub struct SimHost;

impl RestoreHost for SimHost {
    fn sys(&self, op: &SysOp) -> Result<SysOk, String> {
        Err(format!("not run in a simulation: {}", op.describe()))
    }
    fn running(&self) -> Vec<String> {
        Vec::new()
    }
    fn write_stream(&self, _: &Path, _: &str, _: &Path) -> std::io::Result<()> {
        Ok(())
    }
    fn simulated(&self) -> bool {
        true
    }
}
