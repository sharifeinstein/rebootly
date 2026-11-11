//! `rebootly-cli volumes | open | close` (SPEC §3.3, §7): list the volumes
//! Windows could be on, unlock BitLocker and mount read-only so `inspect`
//! and `backup --windows-root` can read them, and undo that afterwards.

use std::path::{Path, PathBuf};

use anyhow::Context;
use clap::Args;
use rebootly_core::install::WindowsInstall;
use rebootly_core::volume::{RecoveryKey, Volume, VolumeState};

/// Recovery key: from a file, `$REBOOTLY_RECOVERY_KEY`, or a hidden prompt.
pub const KEY_ENV: &str = "REBOOTLY_RECOVERY_KEY";

#[derive(Args)]
pub struct VolumesArgs {
    /// Print JSON instead of text.
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
pub struct OpenArgs {
    /// Device or drive letter from `rebootly-cli volumes` (`/dev/sda3`, `D:`).
    #[arg(required_unless_present = "all")]
    device: Option<String>,
    /// Open every unmounted Windows (NTFS) volume (Linux; locked ones are
    /// listed, they need a key each).
    #[arg(long, conflicts_with = "device")]
    all: bool,
    /// File holding the 48-digit BitLocker recovery key.
    #[arg(long, value_name = "FILE")]
    recovery_key_file: Option<PathBuf>,
}

fn row(v: &Volume) -> String {
    let state = match &v.state {
        VolumeState::Mounted { root, read_only } => {
            let ro = if *read_only { ", read-only" } else { "" };
            let win = if WindowsInstall::from_path(root).is_some() {
                ", Windows installed"
            } else {
                ""
            };
            format!("at {}{ro}{win}", root.display())
        }
        VolumeState::Locked => "locked: rebootly-cli open <device>".into(),
        VolumeState::Unmounted => "not mounted: rebootly-cli open <device>".into(),
    };
    format!("{}\n    {state}", v.describe())
}

pub fn list(args: VolumesArgs) -> anyhow::Result<u8> {
    let vols = rebootly_host::volumes::list();
    if args.json {
        println!("{}", serde_json::to_string_pretty(&vols)?);
        return Ok(0);
    }
    if vols.is_empty() {
        println!("No NTFS, exFAT or BitLocker volumes found.");
    }
    for v in &vols {
        println!("{}", row(v));
    }
    Ok(0)
}

fn key(file: Option<&Path>) -> anyhow::Result<RecoveryKey> {
    let raw = if let Some(f) = file {
        std::fs::read_to_string(f)
            .with_context(|| format!("reading recovery key file {}", f.display()))?
    } else if let Ok(s) = std::env::var(KEY_ENV) {
        s
    } else {
        eprintln!(
            "The recovery key is 48 digits (8 groups of 6). Find it at aka.ms/myrecoverykey \
             or on the printout/USB saved when BitLocker was turned on."
        );
        let s = rpassword::prompt_password("Recovery key: ")?;
        let n = s.chars().filter(char::is_ascii_digit).count();
        if n != 48 {
            anyhow::bail!(
                "the prompt received {n} digits, a recovery key has 48 (if you pasted it, type \
                 it instead, or use --recovery-key-file)"
            );
        }
        s
    };
    Ok(RecoveryKey::parse(&raw)?)
}

pub fn open(args: OpenArgs) -> anyhow::Result<u8> {
    let mut vols = rebootly_host::volumes::Volumes::new();
    vols.keep(); // stays open for the next commands; `close` undoes it
    if args.all {
        for n in vols.mount_all() {
            eprintln!("{n}");
        }
        for v in rebootly_host::volumes::list() {
            println!("{}", row(&v));
        }
        return Ok(0);
    }
    let dev = args.device.unwrap_or_default();
    let locked = rebootly_host::volumes::list()
        .iter()
        .any(|v| v.device.eq_ignore_ascii_case(&dev) && v.is_locked());
    let key = if locked {
        Some(key(args.recovery_key_file.as_deref())?)
    } else {
        None
    };
    let (root, notes) = vols.open(&dev, key.as_ref())?;
    for n in notes {
        eprintln!("{n}");
    }
    println!("{dev} can be read at {}", root.display());
    if WindowsInstall::from_path(&root).is_some() {
        println!(
            "Windows is installed there: rebootly-cli backup --windows-root {} --target <DIR>",
            root.display()
        );
    }
    Ok(0)
}

/// Unmount and lock again what `open` left (Linux).
pub fn close() -> anyhow::Result<u8> {
    #[cfg(not(windows))]
    {
        let errs = rebootly_linux::mount::close_all(&rebootly_linux::cmd::System);
        for e in &errs {
            eprintln!("{e}");
        }
        Ok(u8::from(!errs.is_empty()))
    }
    #[cfg(windows)]
    {
        println!(
            "Nothing to do on Windows: drives stay mounted, and an unlocked BitLocker drive \
             locks again at restart (or: manage-bde -lock X:)."
        );
        Ok(0)
    }
}
