//! Manual steps that come from packs (notes, `post_restore` commands).

use crate::backup::manifest::{ManualStep, PackRecord};
use crate::packs::schema::Mode;

use super::system::name_list;

/// Pack notes and `post_restore` commands, as manual steps.
pub(super) fn pack_steps(rec: &PackRecord) -> Vec<ManualStep> {
    let mut out = Vec::new();
    if let Some(n) = &rec.notes {
        out.push(ManualStep {
            title: rec.name.clone(),
            detail: n.clone(),
        });
    }
    for pr in &rec.post_restore {
        let mut detail = format!("Run: {}", pr.run);
        if let Some(var) = pr
            .run
            .split("{each:")
            .nth(1)
            .and_then(|s| s.split('}').next())
        {
            let names: Vec<String> = rec
                .paths
                .iter()
                .filter(|p| p.mode == Mode::ListOnly)
                .filter(|p| {
                    p.template
                        .rsplit('\\')
                        .next()
                        .is_some_and(|l| l.eq_ignore_ascii_case(var))
                })
                .flat_map(|p| p.listed.iter().map(|n| strip_version(n)))
                .collect::<std::collections::BTreeSet<String>>()
                .into_iter()
                .collect();
            if !names.is_empty() {
                detail = format!(
                    "Run `{}` for each of: {}",
                    pr.run.replace(&format!("{{each:{var}}}"), "<name>"),
                    name_list(&names, 30)
                );
            }
        }
        out.push(ManualStep {
            title: format!("After restore: {}", rec.name),
            detail,
        });
    }
    out
}

/// `publisher.name-1.2.3[-win32-x64]` (an extension folder) → `publisher.name`.
pub fn strip_version(folder: &str) -> String {
    let start = folder.find('.').map_or(0, |i| i + 1);
    let b = folder.as_bytes();
    (start..b.len().saturating_sub(1))
        .find(|&i| b[i] == b'-' && b[i + 1].is_ascii_digit())
        .map_or_else(|| folder.to_owned(), |i| folder[..i].to_owned())
}
