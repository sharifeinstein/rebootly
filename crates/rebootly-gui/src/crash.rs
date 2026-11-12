//! `Rebootly.exe` has no console, so a crash would otherwise vanish without a
//! word (owner, 2026-11-12: "it just does nothing"). The panic hook writes
//! `crash.txt` beside the donation marker and, on the main thread, says so
//! in a message box. Worker-thread panics are still handled by their pages.

use std::path::{Path, PathBuf};

/// `%LOCALAPPDATA%\Rebootly\crash.txt` (Windows), or
/// `~/.config/rebootly/crash.txt`.
pub fn log_path() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("LOCALAPPDATA") {
        return Some(PathBuf::from(d).join("Rebootly").join("crash.txt"));
    }
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config/rebootly/crash.txt"))
}

/// The text written to `crash.txt`.
pub fn report(version: &str, thread: &str, panic: &str, backtrace: &str) -> String {
    format!(
        "Rebootly {version} stopped unexpectedly.\n\
         Thread: {thread}\n\
         System: {os}\n\n\
         {panic}\n\n\
         Backtrace:\n{backtrace}\n",
        os = std::env::consts::OS,
    )
}

/// Write the report (replacing an older one). Returns where it went.
pub fn write(path: &Path, text: &str) -> Option<PathBuf> {
    std::fs::create_dir_all(path.parent()?).ok()?;
    std::fs::write(path, text).ok()?;
    Some(path.to_path_buf())
}

/// Message for the box shown when the main thread panics.
pub fn message(panic: &str, written: Option<&Path>) -> String {
    let saved = match written {
        Some(p) => format!("Details were saved to:\n{}", p.display()),
        None => "The details couldn't be saved.".into(),
    };
    format!(
        "Rebootly stopped unexpectedly:\n{panic}\n\n{saved}\n\n\
         The text version works in any console: rebootly-cli.exe"
    )
}

/// Install the hook. `show` displays a message (a box on Windows) and is
/// called only for the main thread, which then exits with code 101.
pub fn install(version: &'static str, show: fn(&str)) {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let name = thread.name().unwrap_or("unnamed");
        let panic = info.to_string();
        let bt = std::backtrace::Backtrace::force_capture().to_string();
        let text = report(version, name, &panic, &bt);
        let written = log_path().and_then(|p| write(&p, &text));
        default(info);
        if name == "main" {
            show(&message(&panic, written.as_deref()));
            std::process::exit(101);
        }
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_and_message() {
        let r = report(
            "0.1.0 (commit abc)",
            "main",
            "panicked at x.rs:1:2:\nboom",
            "  0: f",
        );
        assert!(r.starts_with("Rebootly 0.1.0 (commit abc) stopped unexpectedly."));
        assert!(r.contains("Thread: main") && r.contains("boom") && r.contains("  0: f"));

        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("Rebootly").join("crash.txt");
        assert_eq!(write(&p, &r).as_deref(), Some(p.as_path()));
        assert_eq!(std::fs::read_to_string(&p).unwrap(), r);

        let m = message("boom", Some(&p));
        assert!(m.contains("Details were saved to:") && m.contains("crash.txt"));
        assert!(message("boom", None).contains("couldn't be saved"));
    }
}
