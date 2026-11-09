//! Environment detection (SPEC §3.1).

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Environment {
    LiveWindows,
    WinPe,
    Linux,
}

impl Environment {
    /// Detect where this process is running.
    ///
    /// WinPE is identified by `HKLM\SYSTEM\CurrentControlSet\Control\MiniNT`.
    /// Core has no Win32 bindings, so it asks `reg.exe` (present in WinPE);
    /// migrate-win may replace this with a direct API call in M4/M7.
    pub fn detect() -> Self {
        #[cfg(windows)]
        {
            let minint = std::process::Command::new("reg")
                .args(["query", r"HKLM\SYSTEM\CurrentControlSet\Control\MiniNT"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|s| s.success());
            if minint {
                Environment::WinPe
            } else {
                Environment::LiveWindows
            }
        }
        #[cfg(not(windows))]
        {
            Environment::Linux
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Environment::LiveWindows => "live-windows",
            Environment::WinPe => "winpe",
            Environment::Linux => "linux",
        }
    }
}

#[cfg(test)]
mod tests {
    #[cfg(not(windows))]
    #[test]
    fn linux_detected_at_compile_time() {
        assert_eq!(super::Environment::detect(), super::Environment::Linux);
    }
}
