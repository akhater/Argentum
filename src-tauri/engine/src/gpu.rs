//! The engine's own GPU device.
//!
//! Not RapidRAW's processor: the engine opens a wgpu device of its own, the
//! first time a GPU tool asks for one, so it runs - and tests - without the
//! app.

use std::sync::{Arc, OnceLock};

use crate::{Error, Result};

pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub adapter: wgpu::AdapterInfo,
}

impl Gpu {
    /// The fastest adapter wgpu finds, with every limit it offers.
    pub fn new() -> Result<Gpu> {
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: None,
        }))
        .map_err(|e| Error::NoGpu(e.to_string()))?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("argentum-engine"),
            required_features: wgpu::Features::empty(),
            required_limits: adapter.limits(),
            experimental_features: wgpu::ExperimentalFeatures::default(),
            // Memory in blocks of 8 to 64 MB rather than 128 to 256 (host:
            // 4 to 32 rather than 64 to 128). On an integrated GPU the
            // blocks are the process's memory, and each kind of resource
            // the pixel pass adds opened a block of its own: 0.17 GB at the
            // peak while a photo is only fitted. A picture larger than a
            // block has memory of its own either way.
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            trace: wgpu::Trace::Off,
        }))
        .map_err(|e| Error::NoGpu(e.to_string()))?;
        Ok(Gpu {
            device,
            queue,
            adapter: adapter.get_info(),
        })
    }
}

/// A device opened on first use, or the reason there is none.
#[derive(Default)]
pub struct GpuSlot(OnceLock<std::result::Result<Arc<Gpu>, String>>);

impl GpuSlot {
    /// Opens `Gpu::new()` the first time it is asked.
    pub fn new() -> Self {
        Self::default()
    }

    /// Hands out this device instead.
    pub fn with(gpu: Arc<Gpu>) -> Self {
        let slot = Self::default();
        let _ = slot.0.set(Ok(gpu));
        slot
    }

    pub fn get(&self) -> Result<Arc<Gpu>> {
        self.0
            .get_or_init(|| Gpu::new().map(Arc::new).map_err(|e| e.to_string()))
            .clone()
            .map_err(Error::NoGpu)
    }
}
