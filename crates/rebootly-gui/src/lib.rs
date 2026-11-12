//! Rebootly's Windows GUI (egui/eframe). The same core and machine layer
//! as the text screens (`rebootly-cli`); every action also has a command.

pub mod app;
pub mod clipboard;
pub mod crash;
pub mod donate;
pub mod pages;
pub mod task;
pub mod widgets;

pub use app::RebootlyApp;

/// Picks the graphics adapter: a real GPU if there is one, else the
/// software (WARP/CPU) adapter, so VMs without 3D still get a window.
pub fn pick_adapter(
    adapters: &[eframe::wgpu::Adapter],
    surface: Option<&eframe::wgpu::Surface<'_>>,
) -> Result<eframe::wgpu::Adapter, String> {
    use eframe::wgpu::DeviceType;
    let rank = |t: DeviceType| match t {
        DeviceType::DiscreteGpu => 0,
        DeviceType::IntegratedGpu => 1,
        DeviceType::VirtualGpu => 2,
        DeviceType::Other => 3,
        DeviceType::Cpu => 4,
    };
    for a in adapters {
        let i = a.get_info();
        crash::trace(&format!(
            "adapter: {} ({:?}, {:?}, driver {} {})",
            i.name, i.backend, i.device_type, i.driver, i.driver_info
        ));
    }
    let chosen = adapters
        .iter()
        .filter(|a| surface.is_none_or(|s| a.is_surface_supported(s)))
        .min_by_key(|a| rank(a.get_info().device_type))
        .cloned()
        .ok_or_else(|| "no graphics adapter can draw this window".to_string());
    match &chosen {
        Ok(a) => crash::trace(&format!("chosen: {}", a.get_info().name)),
        Err(e) => crash::trace(e),
    }
    chosen
}

/// Graphics backends: `WGPU_BACKEND` if set, else DirectX 12 only on
/// Windows (Vulkan and OpenGL drivers and overlay layers can crash the
/// process before a window exists), the usual set elsewhere.
pub fn backends() -> eframe::wgpu::Backends {
    use eframe::wgpu::Backends;
    Backends::from_env().unwrap_or(if cfg!(windows) {
        Backends::DX12
    } else {
        Backends::PRIMARY | Backends::GL
    })
}
