//! Live Tier 4 captures by command (SPEC §4 Tier 4): `winget export`,
//! `netsh wlan export profile key=clear` (only when the encrypted bundle is
//! available; the clear-text XMLs go nowhere else) and the printer list.
//! Outputs land in a private temp folder that is removed on drop.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use rebootly_core::{Capture, Captures};

const TIMEOUT: Duration = Duration::from_secs(180);

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
        winget(&dir, &mut out);
        printers(&dir, &mut out);
        if sensitive {
            wifi(&dir, &mut out);
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

/// Run with a timeout; stdout returned on exit code 0.
fn run_cmd(cmd: &mut Command) -> Result<Vec<u8>, String> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("not available ({e})"))?;
    let mut stdout = child.stdout.take();
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(s) = stdout.as_mut() {
            let _ = std::io::Read::read_to_end(s, &mut buf);
        }
        buf
    });
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let buf = reader.join().unwrap_or_default();
                return if status.success() {
                    Ok(buf)
                } else {
                    Err(format!("failed ({status})"))
                };
            }
            Ok(None) if start.elapsed() > TIMEOUT => {
                let _ = child.kill();
                return Err(format!("timed out after {}s", TIMEOUT.as_secs()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(200)),
            Err(e) => return Err(e.to_string()),
        }
    }
}

fn push(out: &mut Captures, dest: &str, file: PathBuf, origin: &str, sensitive: bool) {
    out.files.push(Capture {
        dest: dest.into(),
        file,
        origin: origin.into(),
        sensitive,
    });
}

fn winget(dir: &Path, out: &mut Captures) {
    let file = dir.join("winget.json");
    let r = run_cmd(
        Command::new("winget")
            .args(["export", "-o"])
            .arg(&file)
            .args([
                "--include-versions",
                "--accept-source-agreements",
                "--disable-interactivity",
            ]),
    );
    // winget exits non-zero when some apps aren't in a source, but still
    // writes the file: keep whatever it wrote.
    if file.is_file() {
        push(out, "system/winget.json", file, "winget export", false);
    } else if let Err(e) = r {
        out.warnings.push(format!(
            "winget export {e}; apps are still listed in system/apps.json"
        ));
    }
}

fn printers(dir: &Path, out: &mut Captures) {
    let script = "Get-Printer | Select-Object Name,DriverName,PortName,Shared,ShareName,Type \
                  | ConvertTo-Json -Depth 2";
    match run_cmd(Command::new("powershell").args([
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        script,
    ])) {
        Ok(json) if !json.is_empty() => {
            let file = dir.join("printers.json");
            if std::fs::write(&file, json).is_ok() {
                push(out, "system/printers.json", file, "Get-Printer", false);
            }
        }
        Ok(_) => {}
        Err(e) => out.warnings.push(format!("printer list {e}")),
    }
}

fn wifi(dir: &Path, out: &mut Captures) {
    let wdir = dir.join("wifi");
    if std::fs::create_dir(&wdir).is_err() {
        return;
    }
    let r = run_cmd(
        Command::new("netsh")
            .args(["wlan", "export", "profile"])
            .arg(format!("folder={}", wdir.display()))
            .arg("key=clear"),
    );
    let mut n = 0;
    if let Ok(rd) = std::fs::read_dir(&wdir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.to_ascii_lowercase().ends_with(".xml") {
                push(
                    out,
                    &format!("system/wifi-export/{name}"),
                    e.path(),
                    "netsh wlan export profile key=clear",
                    true,
                );
                n += 1;
            }
        }
    }
    if n == 0 {
        if let Err(e) = r {
            out.warnings.push(format!(
                "Wi-Fi export {e} (no Wi-Fi adapter or WLAN service off?)"
            ));
        }
    }
}
