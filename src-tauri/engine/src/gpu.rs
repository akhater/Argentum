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

    /// The GPU memory wgpu holds for this device now, in bytes: every
    /// texture and buffer, and wgpu's own staging copies. `None` on a backend
    /// that does not say (OpenGL).
    pub fn allocated_bytes(&self) -> Option<usize> {
        self.device
            .generate_allocator_report()
            .map(|r| r.total_allocated_bytes as usize)
    }
}

/// A device opened on first use, or the reason there is none. A clone
/// shares it.
#[derive(Default, Clone)]
pub struct GpuSlot(Arc<OnceLock<std::result::Result<Arc<Gpu>, Error>>>);

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
        self.0.get_or_init(|| Gpu::new().map(Arc::new)).clone()
    }

    /// The device, if it has been opened; never opens it.
    pub fn opened(&self) -> Option<Arc<Gpu>> {
        self.0.get()?.as_ref().ok().cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slot_that_failed_to_open_keeps_its_error_unchanged() {
        let slot = GpuSlot::default();
        let _ = slot.0.set(Err(Error::NoGpu("no adapter".into())));
        let Err(err) = slot.get() else {
            panic!("opened a device")
        };
        let err = err.to_string();
        assert_eq!(err.matches("no GPU").count(), 1, "{err}");
    }
}
