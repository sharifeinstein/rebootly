//! Puts the git commit into `rebootly --version`. Reads `.git` directly, so
//! no `git` program is needed; prints `unknown` outside a checkout.

use std::path::{Path, PathBuf};

fn main() {
    let git = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.git");
    let head = git.join("HEAD");
    println!("cargo:rerun-if-changed={}", head.display());
    println!(
        "cargo:rerun-if-changed={}",
        git.join("packed-refs").display()
    );
    let sha = std::fs::read_to_string(&head).ok().and_then(|h| {
        let h = h.trim();
        match h.strip_prefix("ref: ") {
            Some(r) => {
                let loose: PathBuf = git.join(r);
                println!("cargo:rerun-if-changed={}", loose.display());
                std::fs::read_to_string(&loose)
                    .ok()
                    .map(|s| s.trim().to_owned())
                    .or_else(|| packed(&git, r))
            }
            None => Some(h.to_owned()),
        }
    });
    let short = sha
        .filter(|s| s.len() >= 7 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .map_or_else(|| "unknown".to_owned(), |s| s[..7].to_owned());
    println!("cargo:rustc-env=REBOOTLY_COMMIT={short}");
}

/// `<sha> refs/heads/main` in `.git/packed-refs`.
fn packed(git: &Path, r: &str) -> Option<String> {
    std::fs::read_to_string(git.join("packed-refs"))
        .ok()?
        .lines()
        .find_map(|l| {
            let (sha, name) = l.split_once(' ')?;
            (name.trim() == r).then(|| sha.to_owned())
        })
}
