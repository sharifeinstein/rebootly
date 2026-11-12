//! Volume discovery on Linux (SPEC §3.3, §7): `lsblk` rows → [`Volume`]s.
//!
//! Only filesystems Windows uses are listed: NTFS, BitLocker, exFAT. A
//! BitLocker partition that has been unlocked (a `crypt` child from
//! cryptsetup) is listed once, with the child's filesystem and mount and
//! the partition's identity, so drive letters still match.

use std::path::{Path, PathBuf};

use rebootly_core::volume::{Volume, VolumeId, VolumeState};
use serde_json::Value;

use crate::cmd::Runner;

/// One row of `lsblk -J -l`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Row {
    pub name: String,
    pub kname: String,
    pub size: Option<u64>,
    pub fstype: Option<String>,
    pub label: Option<String>,
    pub mountpoint: Option<String>,
    pub kind: String,
    pub removable: bool,
    pub partuuid: Option<String>,
    pub ptuuid: Option<String>,
    pub pttype: Option<String>,
    pub pkname: Option<String>,
    /// Partition start in 512-byte sectors, when blkid gave it.
    pub start: Option<u64>,
}

pub const LSBLK_ARGS: &[&str] = &[
    "-J",
    "-l",
    "-b",
    "-p",
    "-o",
    "NAME,KNAME,SIZE,FSTYPE,LABEL,MOUNTPOINT,TYPE,RM,PARTUUID,PTUUID,PTTYPE,PKNAME",
];

fn text(v: &Value, k: &str) -> Option<String> {
    match v.get(k)? {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Parse `lsblk -J -l …`. Older lsblk writes numbers and booleans as
/// strings ("1", "0"); both forms are accepted.
pub fn parse(json: &str) -> Result<Vec<Row>, serde_json::Error> {
    let v: Value = serde_json::from_str(json)?;
    let rows = v
        .get("blockdevices")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Ok(rows
        .iter()
        .map(|r| Row {
            name: text(r, "name").unwrap_or_default(),
            kname: text(r, "kname")
                .or_else(|| text(r, "name"))
                .unwrap_or_default(),
            size: text(r, "size").and_then(|s| s.parse().ok()),
            fstype: text(r, "fstype"),
            label: text(r, "label"),
            mountpoint: text(r, "mountpoint"),
            kind: text(r, "type").unwrap_or_default(),
            removable: match r.get("rm") {
                Some(Value::Bool(b)) => *b,
                Some(Value::String(s)) => s == "1",
                Some(Value::Number(n)) => n.as_u64() == Some(1),
                _ => false,
            },
            partuuid: text(r, "partuuid"),
            ptuuid: text(r, "ptuuid"),
            pttype: text(r, "pttype"),
            pkname: text(r, "pkname"),
            start: None,
        })
        .collect())
}

/// Fill what lsblk left empty from `blkid -p -o export <dev>` (a system
/// without udev, e.g. a minimal rescue system, gives lsblk no filesystem
/// types, labels or partition UUIDs; blkid probes the device itself).
pub fn fill_from_blkid(row: &mut Row, export: &str) {
    let get = |k: &str| {
        export
            .lines()
            .find_map(|l| l.strip_prefix(k)?.strip_prefix('='))
            .map(str::to_owned)
            .filter(|v| !v.is_empty())
    };
    row.fstype = row.fstype.take().or_else(|| get("TYPE"));
    row.label = row.label.take().or_else(|| get("LABEL"));
    row.pttype = row.pttype.take().or_else(|| get("PART_ENTRY_SCHEME"));
    row.partuuid = row.partuuid.take().or_else(|| get("PART_ENTRY_UUID"));
    if row.ptuuid.is_none() && row.pttype.as_deref() == Some("dos") {
        // MBR PART_ENTRY_UUID is `<disk signature>-<partition number>`.
        row.ptuuid = row
            .partuuid
            .as_deref()
            .and_then(|u| u.split('-').next())
            .map(str::to_owned);
    }
    row.start = row
        .start
        .or_else(|| get("PART_ENTRY_OFFSET").and_then(|o| o.parse().ok()));
}

/// Partition identity as Windows' `MountedDevices` records it. MBR needs
/// the partition's byte offset: `start` (512-byte sectors, from sysfs).
pub fn identity(row: &Row, start: Option<u64>) -> Option<VolumeId> {
    match row.pttype.as_deref()? {
        "gpt" => row.partuuid.as_deref().map(VolumeId::gpt),
        "dos" => Some(VolumeId::Mbr {
            signature: u32::from_str_radix(row.ptuuid.as_deref()?, 16).ok()?,
            offset: start? * 512,
        }),
        _ => None,
    }
}

/// `/sys/class/block/<kname>/start`.
pub fn sysfs_start(kname: &str) -> Option<u64> {
    let base = Path::new(kname).file_name()?;
    std::fs::read_to_string(Path::new("/sys/class/block").join(base).join("start"))
        .ok()?
        .trim()
        .parse()
        .ok()
}

fn wanted(fs: &str) -> bool {
    matches!(
        fs.to_ascii_lowercase().as_str(),
        "ntfs" | "bitlocker" | "exfat"
    )
}

/// Turn rows into volumes. `read_only(mountpoint)` reports how a mount is
/// mounted (from /proc/self/mountinfo); `start(kname)` reads sysfs.
pub fn volumes(
    rows: &[Row],
    read_only: impl Fn(&str) -> bool,
    start: impl Fn(&str) -> Option<u64>,
) -> Vec<Volume> {
    let mut out = Vec::new();
    for r in rows.iter().filter(|r| r.kind != "crypt") {
        let Some(fs) = r.fstype.as_deref().filter(|f| wanted(f)) else {
            continue;
        };
        let bitlocker = fs.eq_ignore_ascii_case("bitlocker");
        // An unlocked BitLocker partition: use its crypt child.
        let child = rows
            .iter()
            .find(|c| c.kind == "crypt" && c.pkname.as_deref() == Some(r.kname.as_str()));
        let shown = child.unwrap_or(r);
        let state = match shown.mountpoint.as_deref() {
            Some(m) => VolumeState::Mounted {
                root: PathBuf::from(m),
                read_only: read_only(m),
            },
            None if bitlocker && child.is_none() => VolumeState::Locked,
            None => VolumeState::Unmounted,
        };
        out.push(Volume {
            device: r.name.clone(),
            label: shown.label.clone().or_else(|| r.label.clone()),
            size: r.size,
            fs: shown.fstype.clone(),
            state,
            id: identity(r, r.start.or_else(|| start(&r.kname))),
            removable: r.removable,
        });
    }
    out
}

/// Is `mountpoint` mounted read-only? (`/proc/self/mountinfo` text.)
pub fn mount_read_only(mountinfo: &str, mountpoint: &str) -> bool {
    mountinfo
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split(' ').collect();
            Some((*f.get(4)?, *f.get(5)?))
        })
        .rfind(|(mp, _)| unescape(mp) == mountpoint)
        .is_some_and(|(_, opts)| opts.split(',').any(|o| o == "ro"))
}

/// mountinfo escapes space, tab, newline and backslash as `\ooo`.
pub(crate) fn unescape(s: &str) -> String {
    s.replace("\\040", " ")
        .replace("\\011", "\t")
        .replace("\\012", "\n")
        .replace("\\134", "\\")
}

/// List the volumes on this machine.
pub fn discover(run: &dyn Runner) -> anyhow::Result<Vec<Volume>> {
    let out = run.run("lsblk", LSBLK_ARGS, None)?;
    if !out.ok() {
        anyhow::bail!("lsblk failed: {}", crate::cmd::last_line(&out.stderr));
    }
    let mut rows = parse(&out.stdout)?;
    for r in rows
        .iter_mut()
        .filter(|r| r.fstype.is_none() && r.kind != "rom")
    {
        if let Ok(o) = run.run("blkid", &["-p", "-o", "export", &r.name], None) {
            // Exit code 2: nothing found on the device (e.g. a bare disk).
            if o.ok() {
                fill_from_blkid(r, &o.stdout);
            }
        }
    }
    let info = std::fs::read_to_string("/proc/self/mountinfo").unwrap_or_default();
    Ok(volumes(&rows, |m| mount_read_only(&info, m), sysfs_start))
}

#[cfg(test)]
#[path = "discover_tests.rs"]
mod tests;
