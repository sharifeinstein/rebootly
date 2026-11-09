//! Passphrase input and `rebootly sensitive list|extract`.

use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use rebootly_core::backup::sensitive::{self, Passphrase};

pub const ENV: &str = "REBOOTLY_PASSPHRASE";

/// From `file`, else `$REBOOTLY_PASSPHRASE`, else a hidden prompt (asked
/// twice when `confirm`). Never echoed or logged.
pub fn passphrase(file: Option<&Path>, confirm: bool) -> anyhow::Result<Passphrase> {
    let raw = if let Some(f) = file {
        let s = std::fs::read_to_string(f)
            .with_context(|| format!("reading passphrase file {}", f.display()))?;
        s.trim_end_matches(['\r', '\n']).to_owned()
    } else if let Ok(s) = std::env::var(ENV) {
        s
    } else {
        let a = rpassword::prompt_password("Passphrase for the sensitive bundle: ")?;
        if confirm {
            let b = rpassword::prompt_password("Same passphrase again: ")?;
            if a != b {
                bail!("the passphrases don't match");
            }
            eprintln!(
                "Keep this passphrase safe: without it sensitive.age can't be opened, \
                 and Rebootly doesn't store it."
            );
        }
        a
    };
    Passphrase::new(raw).map_err(anyhow::Error::msg)
}

#[derive(clap::Subcommand)]
pub enum SensitiveCmd {
    /// List what an encrypted bundle holds.
    List {
        /// The bundle (`sensitive.age` in a backup folder).
        bundle: PathBuf,
        /// Read the passphrase from this file (first line).
        #[arg(long, value_name = "FILE")]
        passphrase_file: Option<PathBuf>,
    },
    /// Decrypt a bundle into a folder and verify every file.
    Extract {
        bundle: PathBuf,
        /// Folder to write into (created if missing).
        #[arg(long, value_name = "DIR")]
        to: PathBuf,
        #[arg(long, value_name = "FILE")]
        passphrase_file: Option<PathBuf>,
    },
}

pub fn run(cmd: SensitiveCmd) -> anyhow::Result<u8> {
    match cmd {
        SensitiveCmd::List {
            bundle,
            passphrase_file,
        } => {
            let pass = passphrase(passphrase_file.as_deref(), false)?;
            let idx = sensitive::list(&bundle, &pass)?;
            for f in &idx.files {
                println!("{:>10}  {}  ({})", f.size, f.path, f.source);
            }
            eprintln!("{} file(s), created {}", idx.files.len(), idx.created);
        }
        SensitiveCmd::Extract {
            bundle,
            to,
            passphrase_file,
        } => {
            let pass = passphrase(passphrase_file.as_deref(), false)?;
            let idx = sensitive::extract(&bundle, &pass, &to)?;
            eprintln!(
                "{} file(s) decrypted and verified into {}. They are keys and credentials: \
                 delete this folder once they're back in place.",
                idx.files.len(),
                to.display()
            );
        }
    }
    Ok(0)
}
