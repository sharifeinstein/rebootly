//! Rebootly's Windows GUI (egui/eframe). The same core and machine layer
//! as the text screens (`rebootly-cli`); every action also has a command.

pub mod app;
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
    adapters
        .iter()
        .filter(|a| surface.is_none_or(|s| a.is_surface_supported(s)))
        .min_by_key(|a| rank(a.get_info().device_type))
        .cloned()
        .ok_or_else(|| "no graphics adapter can draw this window".to_string())
}
