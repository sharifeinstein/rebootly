//! Plain-language pre-flight summary (SPEC §11): "47.2 GiB, about 38 min to
//! this drive, 12 apps to reinstall, 3 things to do by hand."

use std::io::Write as _;
use std::path::Path;
use std::time::{Duration, Instant};

use super::plan::Plan;
use super::preflight::{Preflight, gib};

/// Size of the write test on the target.
const PROBE_BYTES: usize = 32 << 20;
const PROBE_FILE: &str = ".rebootly-speedtest.tmp";

/// Write speed of the target folder in bytes/s: writes and fsyncs a 32 MiB
/// file, then deletes it. `None` if the folder can't be written.
pub fn speed_probe(dir: &Path) -> Option<f64> {
    let path = dir.join(PROBE_FILE);
    let run = || -> std::io::Result<Duration> {
        let buf = vec![0x5au8; 1 << 20];
        let start = Instant::now();
        let mut f = std::fs::File::create(&path)?;
        for _ in 0..PROBE_BYTES / buf.len() {
            f.write_all(&buf)?;
        }
        f.sync_all()?;
        Ok(start.elapsed())
    };
    let took = run();
    let _ = std::fs::remove_file(&path);
    let secs = took.ok()?.as_secs_f64().max(0.001);
    Some(PROBE_BYTES as f64 / secs)
}

/// "about 38 min", "about 2 h 10 min", "under a minute".
pub fn duration_words(d: Duration) -> String {
    let mins = (d.as_secs() + 30) / 60;
    match mins {
        0 => "under a minute".into(),
        m if m < 60 => format!("about {m} min"),
        m if m % 60 == 0 => format!("about {} h", m / 60),
        m => format!("about {} h {} min", m / 60, m % 60),
    }
}

/// Estimated copy time. Small files cost more than their bytes, so each
/// file adds a fixed overhead.
pub fn estimate(bytes: u64, files: u64, speed: f64) -> Duration {
    const PER_FILE: f64 = 0.002;
    let secs = bytes as f64 / speed.max(1.0) + files as f64 * PER_FILE;
    Duration::from_secs_f64(secs)
}

/// Machine-wide apps listed in the generated `system/apps.json`.
pub fn apps_to_reinstall(plan: &Plan) -> usize {
    plan.generated
        .iter()
        .find(|g| g.dest == "system/apps.json")
        .and_then(|g| serde_json::from_slice::<serde_json::Value>(&g.bytes).ok())
        .and_then(|v| v.get("machine").and_then(|m| m.as_array()).map(Vec::len))
        .unwrap_or(0)
}

#[derive(Debug, Clone)]
pub struct Summary {
    /// One sentence for the top of the screen.
    pub headline: String,
    pub apps: usize,
    pub manual_steps: usize,
    pub eta: Option<Duration>,
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

pub fn summarize(plan: &Plan, pre: &Preflight, speed: Option<f64>) -> Summary {
    let todo_bytes = pre.total_bytes.saturating_sub(pre.resumed_bytes);
    let eta = speed.map(|s| estimate(todo_bytes, pre.total_files, s));
    let apps = apps_to_reinstall(plan);
    let manual = plan.manual_steps.len();
    let mut parts = vec![format!(
        "{} in {}",
        gib(pre.total_bytes),
        plural(pre.total_files as usize, "file", "files")
    )];
    if let Some(d) = eta {
        parts.push(format!("{} to this drive", duration_words(d)));
    }
    if apps > 0 {
        parts.push(format!("{} to reinstall", plural(apps, "app", "apps")));
    }
    parts.push(match manual {
        0 => "nothing to do by hand".into(),
        n => format!("{} to do by hand", plural(n, "thing", "things")),
    });
    let mut headline = parts.join(", ");
    headline.push('.');
    Summary {
        headline,
        apps,
        manual_steps: manual,
        eta,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words() {
        assert_eq!(duration_words(Duration::from_secs(20)), "under a minute");
        assert_eq!(duration_words(Duration::from_secs(38 * 60)), "about 38 min");
        assert_eq!(duration_words(Duration::from_secs(7200)), "about 2 h");
        assert_eq!(
            duration_words(Duration::from_secs(7800)),
            "about 2 h 10 min"
        );
    }

    #[test]
    fn estimate_counts_files() {
        let d = estimate(100 << 20, 1000, (100 << 20) as f64);
        assert!((d.as_secs_f64() - 3.0).abs() < 0.01);
    }

    #[test]
    fn probe_cleans_up() {
        let t = tempfile::tempdir().unwrap();
        assert!(speed_probe(t.path()).unwrap() > 0.0);
        assert!(!t.path().join(PROBE_FILE).exists());
        assert!(speed_probe(&t.path().join("missing")).is_none());
    }
}
