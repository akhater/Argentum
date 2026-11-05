//! The GPU backend of a headless export: Vulkan, then Metal, then DirectX 12,
//! then OpenGL - the first this computer has - whatever the editor's saved
//! setting says. Ours.
//!
//! On DirectX 12, wgpu compiles every pipeline with FXC, the compiler in
//! every Windows, on the CPU and with nothing kept between launches: whatever
//! the graphics card, every launch pays it again (15 s on one test machine,
//! 2026-11-05). Shoebox starts one export per batch, so a Windows user whose
//! editor runs on DirectX 12 - or on OpenGL after a crash fallback - paid it
//! for every batch. Vulkan and Metal hand their shaders to the driver, which
//! keeps its own cache. The pictures are the same within the look bar on
//! every backend (mods/export_at_size.rs). The editor's setting is not
//! touched, and a computer with none of the first ones still gets OpenGL.
//!
//! `WGPU_BACKEND`, when set from outside, still decides.

use std::sync::atomic::{AtomicBool, Ordering};

static HEADLESS: AtomicBool = AtomicBool::new(false);

/// The order a headless export tries the backends in.
const ORDER: [wgpu::Backends; 4] = [
    wgpu::Backends::VULKAN,
    wgpu::Backends::METAL,
    wgpu::Backends::DX12,
    wgpu::Backends::GL,
];

/// This launch is a headless export: it picks its own backend.
pub fn mark_headless() {
    HEADLESS.store(true, Ordering::Release);
}

/// Whether this launch picks its own backend.
pub fn picks_its_own() -> bool {
    HEADLESS.load(Ordering::Acquire) && std::env::var_os("WGPU_BACKEND").is_none()
}

/// The first backend in `ORDER` with an adapter, its instance and adapter.
pub fn first_adapter() -> Option<(wgpu::Instance, wgpu::Adapter)> {
    ORDER.into_iter().find_map(|backends| {
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle_from_env();
        desc.backends = backends;
        let instance = wgpu::Instance::new(desc);
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            ..Default::default()
        }))
        .ok()?;
        log::info!("Headless export runs on {:?}", adapter.get_info().backend);
        Some((instance, adapter))
    })
}
