//! The GPU backend of a headless export: Vulkan, then DirectX 12, then
//! OpenGL, whatever the editor's saved setting says. Ours.
//!
//! On DirectX 12, wgpu compiles every pipeline with FXC, the compiler in
//! every Windows, and building the processor took 15 s (Intel Arc 140V,
//! 2026-11-05), at every launch. Shoebox starts one export per batch, so a
//! user whose editor runs on DirectX 12 - or on OpenGL after a crash
//! fallback - paid it for every batch. Vulkan builds the same processor in
//! well under a second, and the export's pictures are the same within the
//! look bar on either. The editor's own setting is not touched.
//!
//! `WGPU_BACKEND`, when set from outside, still decides.

use std::sync::atomic::{AtomicBool, Ordering};

static HEADLESS: AtomicBool = AtomicBool::new(false);

/// The order a headless export tries the backends in.
const ORDER: [wgpu::Backends; 3] = [
    wgpu::Backends::VULKAN,
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
