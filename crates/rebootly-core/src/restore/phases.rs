//! Phases 5–8: app packs, registry import, system settings, Explorer restart. Each
//! change is journalled for undo before it is made.

use std::path::{Path, PathBuf};

use super::conflict::Category;
use super::files::restore_file;
use super::host::{SysOk, SysOp};
use super::map::FileAction;
use super::run::{Event, Exec, Phase, Status};
use super::system::SysAction;
use super::undo::UndoEntry;

pub(super) fn registry_phase(ex: &mut Exec<'_>) {
    if ex.plan.registry.is_empty() {
        return;
    }
    ex.ui.event(Event::Phase(Phase::Registry));
    for r in &ex.plan.registry {
        let what = format!("{} ({} keys)", r.from, r.keys.len());
        if ex.dry_run || ex.host.simulated() {
            ex.sys(
                Phase::Registry,
                &what,
                SysOp::RegImport(PathBuf::from(&r.from)),
            );
            continue;
        }
        if let Err(e) = import_reg(ex, r) {
            ex.push(Phase::Registry, &what, Status::Failed, e);
        } else {
            ex.push(
                Phase::Registry,
                &what,
                Status::Done,
                format!("imported; {} profile path(s) rewritten", r.rewrites),
            );
        }
    }
}

fn import_reg(ex: &mut Exec<'_>, r: &super::plan::RegAction) -> Result<(), String> {
    for key in &r.top {
        let (out, rel) = ex
            .journal()
            .and_then(|j| j.slot("reg"))
            .map_err(|e| e.to_string())?;
        let saved = match ex.host.sys(&SysOp::RegExport {
            key: key.clone(),
            out,
        })? {
            SysOk::No => None,
            _ => Some(rel),
        };
        ex.journal()
            .and_then(|j| {
                j.append(&UndoEntry::Reg {
                    key: key.clone(),
                    saved,
                })
            })
            .map_err(|e| e.to_string())?;
    }
    let (file, _) = ex
        .journal()
        .and_then(|j| j.slot("reg"))
        .map_err(|e| e.to_string())?;
    std::fs::write(&file, super::regcheck::encode(&r.text)).map_err(|e| e.to_string())?;
    ex.host.sys(&SysOp::RegImport(file)).map(|_| ())
}

pub(super) fn system_phase(ex: &mut Exec<'_>) {
    let has_files = ex.plan.files.iter().any(|f| f.category == Category::System);
    if ex.plan.system.is_empty() && !has_files {
        return;
    }
    ex.ui.event(Event::Phase(Phase::System));
    for f in ex
        .plan
        .files
        .iter()
        .filter(|f| f.category == Category::System)
    {
        restore_file(ex, Phase::System, f);
    }
    for a in &ex.plan.system {
        let what = a.describe();
        let (check, entry, op) = match a {
            SysAction::Wifi { file, name } => (
                Some(SysOp::WifiExists(name.clone())),
                UndoEntry::Wifi { name: name.clone() },
                SysOp::WifiAdd(file.clone()),
            ),
            SysAction::Cert {
                file,
                thumbprint,
                password,
            } => (
                Some(SysOp::CertExists(thumbprint.clone())),
                UndoEntry::Cert {
                    thumbprint: thumbprint.clone(),
                },
                SysOp::CertImport {
                    file: file.clone(),
                    password: password.clone(),
                },
            ),
            SysAction::Task { name, file } => (
                Some(SysOp::TaskExists(name.clone())),
                UndoEntry::Task { name: name.clone() },
                SysOp::TaskCreate {
                    name: name.clone(),
                    xml: file.clone(),
                },
            ),
            SysAction::Power { file } => {
                power(ex, &what, file);
                continue;
            }
            SysAction::Winget { .. } => continue,
        };
        if check.and_then(|c| ex.query(c)) == Some(SysOk::Yes) {
            ex.push(Phase::System, &what, Status::Skipped, "already on this PC");
            continue;
        }
        if !ex.dry_run && !ex.host.simulated() {
            if let Err(e) = ex.journal().and_then(|j| j.append(&entry)) {
                ex.push(
                    Phase::System,
                    &what,
                    Status::Failed,
                    format!("undo journal: {e}"),
                );
                continue;
            }
        }
        if ex.sys(Phase::System, &what, op).is_some() {
            ex.push(Phase::System, &what, Status::Done, "");
        }
    }
}

fn power(ex: &mut Exec<'_>, what: &str, file: &Path) {
    if ex.dry_run || ex.host.simulated() {
        ex.sys(Phase::System, what, SysOp::PowerImport(file.to_path_buf()));
        return;
    }
    let previous = match ex.host.sys(&SysOp::PowerActive) {
        Ok(SysOk::Value(g)) => Some(g),
        _ => None,
    };
    let Some(SysOk::Value(guid)) =
        ex.sys(Phase::System, what, SysOp::PowerImport(file.to_path_buf()))
    else {
        return;
    };
    if let Err(e) = ex.journal().and_then(|j| {
        j.append(&UndoEntry::Power {
            imported: guid.clone(),
            previous,
        })
    }) {
        ex.push(
            Phase::System,
            what,
            Status::Failed,
            format!("undo journal: {e}"),
        );
        return;
    }
    if ex
        .sys(Phase::System, what, SysOp::PowerSetActive(guid.clone()))
        .is_some()
    {
        ex.push(
            Phase::System,
            what,
            Status::Done,
            format!("active plan is now {guid}"),
        );
    }
}

/// Phase 8: only when shell settings changed.
pub(super) fn explorer(ex: &mut Exec<'_>) {
    let changed = ex.shell_changed
        || ex
            .res
            .outcomes
            .iter()
            .any(|o| o.status == Status::Done && o.phase == Phase::Registry);
    if ex.plan.restart_explorer && changed {
        ex.ui.event(Event::Phase(Phase::Explorer));
        if ex
            .sys(Phase::Explorer, "Explorer", SysOp::RestartExplorer)
            .is_some()
        {
            ex.push(
                Phase::Explorer,
                "Explorer",
                Status::Done,
                "restarted so the settings take effect",
            );
        }
    }
}

/// Phase 5: per pack, only while its app is closed.
pub(super) fn packs_phase(ex: &mut Exec<'_>) {
    if !ex.plan.files.iter().any(|f| f.category == Category::Packs) {
        return;
    }
    ex.ui.event(Event::Phase(Phase::Packs));
    let running: Vec<String> = ex
        .host
        .running()
        .into_iter()
        .map(|p| p.to_ascii_lowercase())
        .collect();
    let total = ex
        .plan
        .files
        .iter()
        .filter(|f| f.category == Category::Packs)
        .count();
    let mut done = 0;
    for p in &ex.plan.packs {
        let open: Vec<&String> = p
            .requires_closed
            .iter()
            .filter(|r| running.contains(&r.to_ascii_lowercase()))
            .collect();
        let files: Vec<&FileAction> = ex
            .plan
            .files
            .iter()
            .filter(|f| f.pack.as_deref() == Some(&p.id))
            .collect();
        if !open.is_empty() {
            let names = open
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            ex.push(
                Phase::Packs,
                &p.name,
                Status::Skipped,
                format!(
                    "{names} is running: close it and run the restore again ({} files)",
                    files.len()
                ),
            );
            done += files.len();
            continue;
        }
        for f in files {
            restore_file(ex, Phase::Packs, f);
            done += 1;
            ex.ui.event(Event::Progress { done, total });
        }
    }
}
