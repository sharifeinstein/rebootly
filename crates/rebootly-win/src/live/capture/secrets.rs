//! Captures holding secrets. They run only when the encrypted bundle is
//! available, write into the private temp folder, and every file they make
//! is marked sensitive, so it goes into `sensitive.age` and nowhere else.

use std::path::Path;
use std::process::Command;

use rebootly_core::Captures;

use super::push;
use crate::live::cmd::{powershell, run_cmd};
use crate::parse::password;

pub fn wifi(dir: &Path, out: &mut Captures) {
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

/// Exports the running user's personal certificates (`Cert:\CurrentUser\My`):
/// with their private key as PFX where the key is exportable, else as CER.
/// Writes `REBOOTLY_DIR\<thumbprint>.pfx|.cer` and prints `NOKEY <subject>`
/// for keys Windows won't let go of.
const CERT_SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
$pw = ConvertTo-SecureString $env:REBOOTLY_PFX -AsPlainText -Force
foreach ($c in Get-ChildItem Cert:\CurrentUser\My) {
  $base = Join-Path $env:REBOOTLY_DIR $c.Thumbprint
  if ($c.HasPrivateKey) {
    try { Export-PfxCertificate -Cert $c -FilePath "$base.pfx" -Password $pw | Out-Null }
    catch {
      Export-Certificate -Cert $c -FilePath "$base.cer" | Out-Null
      [Console]::Out.WriteLine("NOKEY $($c.Subject)")
    }
  } else { Export-Certificate -Cert $c -FilePath "$base.cer" | Out-Null }
}
"#;

/// Certificates of the user the tool runs as (other users' keys are
/// protected by their own sign-in). The PFX password is random, kept only
/// inside the bundle next to the PFX files.
pub fn certificates(dir: &Path, out: &mut Captures) {
    let cdir = dir.join("certs");
    if std::fs::create_dir(&cdir).is_err() {
        return;
    }
    let mut random = [0u8; 24];
    if getrandom::fill(&mut random).is_err() {
        return out
            .warnings
            .push("certificates: no random source for the PFX password".into());
    }
    let pw = password(&random);
    let mut cmd = powershell(CERT_SCRIPT);
    cmd.env("REBOOTLY_PFX", &pw).env("REBOOTLY_DIR", &cdir);
    let r = run_cmd(&mut cmd);
    let mut n = 0;
    if let Ok(rd) = std::fs::read_dir(&cdir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            push(
                out,
                &format!("system/certs/{name}"),
                e.path(),
                "certificate export",
                true,
            );
            n += 1;
        }
    }
    match r {
        Ok(stdout) => {
            for line in String::from_utf8_lossy(&stdout).lines() {
                if let Some(subj) = line.strip_prefix("NOKEY ") {
                    out.warnings.push(format!(
                        "certificate {subj}: its private key can't be exported; \
                         re-issue or re-enrol it on the new PC"
                    ));
                }
            }
        }
        Err(e) => out.warnings.push(format!("certificate export {e}")),
    }
    if n > 0 {
        let pfile = cdir.join("PFX-PASSWORD.txt");
        if std::fs::write(&pfile, format!("{pw}\r\n")).is_ok() {
            push(
                out,
                "system/certs/PFX-PASSWORD.txt",
                pfile,
                "random PFX password",
                true,
            );
        }
    }
}
