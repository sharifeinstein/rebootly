//! `Rebootly.exe`: the window. No console in release builds.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::sync::Arc;

use eframe::egui;
use eframe::egui_wgpu::{WgpuConfiguration, WgpuSetup, WgpuSetupCreateNew};
use rebootly_host::{Machine, MachineOptions};

const VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (commit ",
    env!("REBOOTLY_COMMIT"),
    ")"
);

fn main() {
    let machine = Machine::new(MachineOptions {
        version: VERSION.to_string(),
        // `Rebootly.exe --simulate-root DIR`: preview a restore off Windows.
        simulate_root: arg_value("--simulate-root"),
        drives: arg_value("--drive").into_iter().collect(),
        packs_dir: arg_value("--packs-dir"),
    });
    let platform: Arc<dyn rebootly_host::Platform> = Arc::new(machine);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Rebootly")
            .with_inner_size([960.0, 700.0])
            .with_min_inner_size([720.0, 520.0]),
        renderer: eframe::Renderer::Wgpu,
        // Built without eframe's clipboard crates: on Windows the shortcuts
        // are handled by `clipboard::translate` (system clipboard paste).
        clipboard_shortcuts: !cfg!(windows),
        wgpu_options: WgpuConfiguration {
            wgpu_setup: WgpuSetup::CreateNew(WgpuSetupCreateNew {
                native_adapter_selector: Some(Arc::new(rebootly_gui::pick_adapter)),
                ..WgpuSetupCreateNew::without_display_handle()
            }),
            ..WgpuConfiguration::default()
        },
        ..eframe::NativeOptions::default()
    };
    let r = eframe::run_native(
        "Rebootly",
        options,
        Box::new(move |cc| {
            Ok(Box::new(rebootly_gui::RebootlyApp::new(
                &cc.egui_ctx,
                platform,
            )))
        }),
    );
    if let Err(e) = r {
        fail(&format!(
            "Rebootly couldn't open its window: {e}\n\nThe text version works in any console: rebootly-cli.exe"
        ));
    }
}

fn arg_value(name: &str) -> Option<std::path::PathBuf> {
    let mut args = std::env::args();
    while let Some(a) = args.next() {
        if a == name {
            return args.next().map(Into::into);
        }
    }
    None
}

/// No console to print to: show a message box on Windows.
fn fail(msg: &str) {
    #[cfg(windows)]
    {
        rfd::MessageDialog::new()
            .set_title("Rebootly")
            .set_description(msg)
            .set_level(rfd::MessageLevel::Error)
            .show();
    }
    #[cfg(not(windows))]
    eprintln!("{msg}");
    std::process::exit(1);
}
