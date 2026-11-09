//! `rebootly pack list | validate` and pack loading for `backup`.

use std::path::{Path, PathBuf};

use anyhow::bail;
use clap::Subcommand;
use rebootly_core::packs::load::{default_dir, load_all, parse_file};
use rebootly_core::packs::schema::CompiledPack;

#[derive(Subcommand)]
pub enum PackCmd {
    /// List available packs (built-in plus the packs folder).
    List {
        #[arg(long, value_name = "DIR")]
        packs_dir: Option<PathBuf>,
    },
    /// Check pack YAML files against the schema before importing them.
    Validate {
        #[arg(required = true, value_name = "FILE")]
        files: Vec<PathBuf>,
    },
}

/// Load packs for a backup. Broken pack files are reported and skipped.
pub fn load(dir: Option<&Path>, only: &[String]) -> anyhow::Result<Vec<CompiledPack>> {
    let dir = dir.map(Path::to_path_buf).or_else(default_dir);
    let (mut packs, errors) = load_all(dir.as_deref());
    for e in &errors {
        eprint!("warning: {e}");
    }
    if !only.is_empty() {
        let unknown: Vec<&String> = only
            .iter()
            .filter(|id| !packs.iter().any(|p| &p.pack.id == *id))
            .collect();
        if !unknown.is_empty() {
            bail!(
                "unknown pack id(s): {:?} (see `rebootly pack list`)",
                unknown
            );
        }
        packs.retain(|p| only.contains(&p.pack.id));
    }
    Ok(packs)
}

pub fn run(cmd: PackCmd) -> anyhow::Result<u8> {
    match cmd {
        PackCmd::List { packs_dir } => {
            for p in load(packs_dir.as_deref(), &[])? {
                println!(
                    "{:<18} v{:<3} {:<42} {}",
                    p.pack.id, p.pack.version, p.pack.name, p.origin
                );
            }
            Ok(0)
        }
        PackCmd::Validate { files } => {
            let mut bad = 0;
            for f in files {
                match parse_file(&f) {
                    Ok(p) => println!("ok   {} ({} v{})", f.display(), p.pack.id, p.pack.version),
                    Err(e) => {
                        bad += 1;
                        print!("{e}");
                    }
                }
            }
            Ok(if bad > 0 { 1 } else { 0 })
        }
    }
}
