//! Running helper commands (winget, netsh, powercfg, PowerShell) with a
//! timeout. Output is never logged: some of it (Wi-Fi export) is secret.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::parse::encode_command;

pub const TIMEOUT: Duration = Duration::from_secs(180);

/// Run with a timeout; stdout returned on exit code 0.
pub fn run_cmd(cmd: &mut Command) -> Result<Vec<u8>, String> {
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

/// Run with the console attached (long jobs whose progress the user should
/// see, like `winget import`), with a timeout.
pub fn run_visible(cmd: &mut Command, timeout: Duration) -> Result<(), String> {
    let mut child = cmd
        .stdin(Stdio::null())
        .spawn()
        .map_err(|e| format!("not available ({e})"))?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => return Err(format!("finished with errors ({status})")),
            Ok(None) if start.elapsed() > timeout => {
                let _ = child.kill();
                return Err(format!("timed out after {} min", timeout.as_secs() / 60));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(500)),
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// `powershell -NoProfile -NonInteractive -EncodedCommand <script>`.
/// Encoded, so quotes in the script survive Windows argument quoting.
pub fn powershell(script: &str) -> Command {
    let mut c = Command::new("powershell");
    c.args(["-NoProfile", "-NonInteractive", "-EncodedCommand"])
        .arg(encode_command(script));
    c
}
