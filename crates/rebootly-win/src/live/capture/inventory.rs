//! Inventory captures: what was installed and configured, no secrets.

use std::path::Path;
use std::process::Command;

use rebootly_core::Captures;

use super::push;
use crate::live::cmd::{powershell, run_cmd};
use crate::parse::find_guid;

pub fn winget(dir: &Path, out: &mut Captures) {
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

pub fn printers(dir: &Path, out: &mut Captures) {
    let script = "Get-Printer | Select-Object Name,DriverName,PortName,Shared,ShareName,Type \
                  | ConvertTo-Json -Depth 2";
    match run_cmd(&mut powershell(script)) {
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

/// The active power plan (`powercfg /export`, restore: `powercfg /import`).
pub fn power_plan(dir: &Path, out: &mut Captures) {
    let scheme = match run_cmd(Command::new("powercfg").arg("/getactivescheme")) {
        Ok(o) => String::from_utf8_lossy(&o).into_owned(),
        Err(e) => return out.warnings.push(format!("power plan {e}")),
    };
    let Some(guid) = find_guid(&scheme) else {
        return out
            .warnings
            .push("power plan: no active scheme reported".into());
    };
    let file = dir.join("power-plan.pow");
    match run_cmd(Command::new("powercfg").arg("/export").arg(&file).arg(guid)) {
        Ok(_) if file.is_file() => {
            push(
                out,
                "system/power/active-scheme.pow",
                file,
                "powercfg /export",
                false,
            );
            let txt = dir.join("power-plan.txt");
            if std::fs::write(&txt, scheme.trim()).is_ok() {
                push(
                    out,
                    "system/power/active-scheme.txt",
                    txt,
                    "powercfg /getactivescheme",
                    false,
                );
            }
        }
        Ok(_) => out.warnings.push("power plan export wrote nothing".into()),
        Err(e) => out.warnings.push(format!("power plan export {e}")),
    }
}

/// Default app associations (report only on restore: Windows 11 blocks
/// setting them silently).
pub fn default_apps(dir: &Path, out: &mut Captures) {
    let file = dir.join("default-apps.xml");
    let r = run_cmd(
        Command::new("dism")
            .args(["/online", "/English"])
            .arg(format!("/Export-DefaultAppAssociations:{}", file.display())),
    );
    if file.is_file() {
        push(
            out,
            "system/default-apps.xml",
            file,
            "dism /Export-DefaultAppAssociations",
            false,
        );
    } else if let Err(e) = r {
        out.warnings.push(format!("default apps export {e}"));
    }
}

/// Third-party driver list, for reference only (drivers are never
/// installed automatically).
pub fn drivers(dir: &Path, out: &mut Captures) {
    match run_cmd(Command::new("pnputil").arg("/enum-drivers")) {
        Ok(o) if !o.is_empty() => {
            let file = dir.join("drivers.txt");
            if std::fs::write(&file, o).is_ok() {
                push(
                    out,
                    "system/drivers.txt",
                    file,
                    "pnputil /enum-drivers",
                    false,
                );
            }
        }
        Ok(_) => {}
        Err(e) => out.warnings.push(format!("driver list {e}")),
    }
}
