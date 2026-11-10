//! Restore on the running Windows (SPEC §5): the user being restored into,
//! and the system commands behind `rebootly_core::restore::RestoreHost`.
//! Output of these commands is never logged (`netsh` shows Wi-Fi keys).

use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use rebootly_core::expand::VarSet;
use rebootly_core::folders::KNOWN_FOLDERS;
use rebootly_core::restore::target::os_from_build;
use rebootly_core::restore::{PathStyle, RestoreHost, SysOk, SysOp, Target};

use super::cmd::{powershell, run_cmd, run_visible};
use super::knownfolder::known_folder_path;
use crate::parse::{
    find_guid, is_thumbprint, ps_quote, quotable, reg_query_number, tasklist_images,
};

const PROFILE: &str = "{5E6C858F-0E22-4760-9AFE-EA3317B67173}";
const WINGET_TIMEOUT: Duration = Duration::from_secs(2 * 3600);

/// The user running Rebootly, as the restore target.
pub fn live_target() -> std::io::Result<Target> {
    let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    let profile = known_folder_path(PROFILE)
        .or_else(|| env("USERPROFILE"))
        .ok_or_else(|| {
            std::io::Error::other("can't find the profile folder of the current user")
        })?;
    let username = env("USERNAME")
        .or_else(|| profile.rsplit('\\').next().map(String::from))
        .unwrap_or_default();
    let folders = KNOWN_FOLDERS
        .iter()
        .filter_map(|k| Some((k.name.to_owned(), known_folder_path(k.guid)?)))
        .collect();
    let mut vars = VarSet::new();
    for (k, v) in std::env::vars() {
        vars.set(&k, v);
    }
    let build = run_cmd(Command::new("reg").args([
        "query",
        r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion",
        "/v",
        "CurrentBuildNumber",
    ]))
    .ok()
    .and_then(|o| reg_query_number(&String::from_utf8_lossy(&o), "CurrentBuildNumber"));
    let system_root = env("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
    Ok(Target {
        machine: env("COMPUTERNAME"),
        os: build.map(os_from_build),
        build,
        username,
        sid: super::token::current_user_sid().ok(),
        public: env("PUBLIC").unwrap_or_else(|| format!(r"{}\Users\Public", &system_root[..2])),
        program_data: env("ProgramData")
            .unwrap_or_else(|| format!(r"{}\ProgramData", &system_root[..2])),
        system_root,
        profile,
        folders,
        vars,
        paths: PathStyle::Native,
    }
    .with_vars())
}

#[derive(Debug, Default)]
pub struct WinRestoreHost;

fn done(r: Result<Vec<u8>, String>) -> Result<SysOk, String> {
    r.map(|_| SysOk::Done)
}

fn yes_no(r: Result<Vec<u8>, String>) -> SysOk {
    if r.is_ok() { SysOk::Yes } else { SysOk::No }
}

/// PowerShell that prints True/False.
fn ps_bool(script: &str) -> Result<SysOk, String> {
    let out = run_cmd(&mut powershell(script))?;
    Ok(
        if String::from_utf8_lossy(&out)
            .trim()
            .eq_ignore_ascii_case("true")
        {
            SysOk::Yes
        } else {
            SysOk::No
        },
    )
}

fn checked<'a>(name: &'a str, what: &str) -> Result<&'a str, String> {
    if quotable(name) {
        Ok(name)
    } else {
        Err(format!(
            "unusual characters in the {what} name; do this one by hand"
        ))
    }
}

fn thumb(t: &str) -> Result<&str, String> {
    if is_thumbprint(t) {
        Ok(t)
    } else {
        Err("not a certificate thumbprint".into())
    }
}

fn guid_of(out: Result<Vec<u8>, String>) -> Result<SysOk, String> {
    let out = String::from_utf8_lossy(&out?).into_owned();
    find_guid(&out)
        .map(|g| SysOk::Value(g.to_owned()))
        .ok_or_else(|| "no power scheme GUID in the output".into())
}

impl RestoreHost for WinRestoreHost {
    fn sys(&self, op: &SysOp) -> Result<SysOk, String> {
        match op {
            SysOp::WingetImport(f) => run_visible(
                Command::new("winget").args(["import", "-i"]).arg(f).args([
                    "--accept-package-agreements",
                    "--accept-source-agreements",
                    "--ignore-unavailable",
                    "--disable-interactivity",
                ]),
                WINGET_TIMEOUT,
            )
            .map(|()| SysOk::Done),
            SysOp::RegExport { key, out } => {
                // Absent only when Windows says so; any other failure stops the import.
                let path = format!("Registry::{key}");
                if ps_bool(&format!("Test-Path -LiteralPath {}", ps_quote(&path)))? == SysOk::No {
                    return Ok(SysOk::No);
                }
                run_cmd(Command::new("reg").args(["export", key]).arg(out).arg("/y"))
                    .map(|_| SysOk::Yes)
            }
            SysOp::RegImport(f) => done(run_cmd(Command::new("reg").arg("import").arg(f))),
            SysOp::RegDelete(k) => done(run_cmd(Command::new("reg").args(["delete", k, "/f"]))),
            SysOp::WifiExists(n) => Ok(yes_no(run_cmd(
                Command::new("netsh")
                    .args(["wlan", "show", "profiles"])
                    .raw_arg(format!("name=\"{}\"", checked(n, "Wi-Fi")?)),
            ))),
            SysOp::WifiAdd(f) => done(run_cmd(
                Command::new("netsh")
                    .args(["wlan", "add", "profile"])
                    .raw_arg(format!("filename=\"{}\"", f.display())),
            )),
            SysOp::WifiDelete(n) => done(run_cmd(
                Command::new("netsh")
                    .args(["wlan", "delete", "profile"])
                    .raw_arg(format!("name=\"{}\"", checked(n, "Wi-Fi")?)),
            )),
            SysOp::PowerActive => {
                guid_of(run_cmd(Command::new("powercfg").arg("/getactivescheme")))
            }
            SysOp::PowerImport(f) => {
                guid_of(run_cmd(Command::new("powercfg").arg("/import").arg(f)))
            }
            SysOp::PowerSetActive(g) => {
                done(run_cmd(Command::new("powercfg").args(["/setactive", g])))
            }
            SysOp::PowerDelete(g) => done(run_cmd(Command::new("powercfg").args(["/delete", g]))),
            SysOp::CertExists(t) => ps_bool(&format!(
                "Test-Path -LiteralPath 'Cert:\\CurrentUser\\My\\{}'",
                thumb(t)?
            )),
            SysOp::CertImport { file, password } => {
                let f = ps_quote(&file.display().to_string());
                let script = match password {
                    Some(pw) => format!(
                        "$ErrorActionPreference='Stop'; \
                         $p = ConvertTo-SecureString -String (Get-Content -LiteralPath {} -Raw).Trim() -AsPlainText -Force; \
                         Import-PfxCertificate -FilePath {f} -CertStoreLocation Cert:\\CurrentUser\\My -Password $p -Exportable | Out-Null",
                        ps_quote(&pw.display().to_string())
                    ),
                    None => format!(
                        "$ErrorActionPreference='Stop'; Import-Certificate -FilePath {f} -CertStoreLocation Cert:\\CurrentUser\\My | Out-Null"
                    ),
                };
                done(run_cmd(&mut powershell(&script)))
            }
            SysOp::CertDelete(t) => done(run_cmd(&mut powershell(&format!(
                "$ErrorActionPreference='Stop'; Remove-Item -LiteralPath 'Cert:\\CurrentUser\\My\\{}'",
                thumb(t)?
            )))),
            SysOp::TaskExists(n) => Ok(yes_no(run_cmd(Command::new("schtasks").args([
                "/query",
                "/tn",
                checked(n, "task")?,
            ])))),
            SysOp::TaskCreate { name, xml } => done(run_cmd(
                Command::new("schtasks")
                    .args(["/create", "/tn", checked(name, "task")?, "/xml"])
                    .arg(xml),
            )),
            SysOp::TaskDelete(n) => done(run_cmd(Command::new("schtasks").args([
                "/delete",
                "/tn",
                checked(n, "task")?,
                "/f",
            ]))),
            SysOp::RestartExplorer => {
                // Windows restarts the shell itself (AutoRestartShell), unelevated.
                // Start it only if that didn't happen: started from here it
                // would run elevated.
                let _ = run_cmd(Command::new("taskkill").args(["/f", "/im", "explorer.exe"]));
                std::thread::sleep(Duration::from_secs(4));
                if self
                    .running()
                    .iter()
                    .any(|p| p.eq_ignore_ascii_case("explorer.exe"))
                {
                    return Ok(SysOk::Done);
                }
                Command::new("explorer.exe")
                    .spawn()
                    .map(|_| SysOk::Done)
                    .map_err(|e| e.to_string())
            }
        }
    }

    fn running(&self) -> Vec<String> {
        run_cmd(Command::new("tasklist").args(["/fo", "csv", "/nh"]))
            .map(|o| tasklist_images(&String::from_utf8_lossy(&o)))
            .unwrap_or_default()
    }

    fn set_attributes(&self, file: &Path, attrs: u32) -> std::io::Result<()> {
        super::attrs::add_attributes(file, attrs)
    }

    fn write_stream(&self, file: &Path, name: &str, from: &Path) -> std::io::Result<()> {
        if name.is_empty() || name.contains([':', '\\', '/']) {
            return Err(std::io::Error::other("bad stream name"));
        }
        let mut target = file.as_os_str().to_owned();
        target.push(":");
        target.push(name);
        let mut src = std::fs::File::open(from)?;
        let mut dst = std::fs::File::create(Path::new(&target))?;
        std::io::copy(&mut src, &mut dst)?;
        dst.sync_all()
    }
}
