//! Commit for the version line (shared with rebootly-cli), and on Windows
//! a manifest: run as administrator (live backup and restore need it),
//! per-monitor DPI awareness, modern common controls.

#[path = "../rebootly-cli/build.rs"]
mod commit;

fn main() {
    commit::main();
    if std::env::var("CARGO_CFG_WINDOWS").is_ok() {
        use embed_manifest::manifest::{DpiAwareness, ExecutionLevel};
        use embed_manifest::{embed_manifest, new_manifest};
        let m = new_manifest("Rebootly")
            .requested_execution_level(ExecutionLevel::RequireAdministrator)
            .dpi_awareness(DpiAwareness::PerMonitorV2);
        embed_manifest(m).expect("embedding the Windows manifest");
    }
    println!("cargo:rerun-if-changed=build.rs");
}
