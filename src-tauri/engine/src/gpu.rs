//! The engine's own GPU device.
//!
//! Not RapidRAW's processor: the engine opens a wgpu device of its own, the
//! first time a GPU tool asks for one, so it runs - and tests - without the
//! app.

use std::fmt;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

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

/// How long a wait for the GPU may take before it is an error.
const WAIT_LIMIT: Duration = Duration::from_secs(120);
/// One slice of a wait. On OpenGL a blocking poll holds the one context lock
/// until the GPU is done, and any other thread that wants the context panics
/// after a second; waiting in short slices lets it go between them. On the
/// other backends a slice ends the moment the GPU is done.
const WAIT_SLICE: Duration = Duration::from_millis(200);

/// Why a wait for the GPU, or a read back from it, failed.
#[derive(Debug)]
pub enum WaitError {
    /// The GPU did not finish within the limit.
    TimedOut(Duration),
    /// wgpu said the wait or the mapping failed.
    Failed(String),
}

impl fmt::Display for WaitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WaitError::TimedOut(limit) => {
                write!(f, "the GPU did not finish within {} s", limit.as_secs())
            }
            WaitError::Failed(m) => f.write_str(m),
        }
    }
}

/// Wait for `index`, or for everything submitted when it is `None`, in short
/// slices, up to the limit.
pub fn wait(
    device: &wgpu::Device,
    index: Option<wgpu::SubmissionIndex>,
) -> std::result::Result<(), WaitError> {
    let started = Instant::now();
    loop {
        match device.poll(wgpu::PollType::Wait {
            submission_index: index.clone(),
            timeout: Some(WAIT_SLICE),
        }) {
            Ok(_) => return Ok(()),
            Err(wgpu::PollError::Timeout) if started.elapsed() < WAIT_LIMIT => {}
            Err(wgpu::PollError::Timeout) => return Err(WaitError::TimedOut(WAIT_LIMIT)),
            Err(e) => return Err(WaitError::Failed(e.to_string())),
        }
    }
}

/// Rows per band for a row of `row_bytes`, so a band is about 64 MB.
pub fn band_rows(row_bytes: usize) -> u32 {
    ((64 << 20) / row_bytes.max(1)).clamp(1, u32::MAX as usize) as u32
}

/// A band of rows mapped from the GPU: each row starts on the copy
/// alignment and holds `tight` bytes of texels, then padding.
pub struct Band<'a> {
    pub bytes: &'a [u8],
    /// Bytes from one row's start to the next.
    pub row: usize,
    /// Bytes of texels in a row.
    pub tight: usize,
    pub rows: usize,
}

impl<'a> Band<'a> {
    /// The rows' texels, padding left out.
    pub fn lines(&self) -> impl Iterator<Item = &'a [u8]> + use<'a> {
        let (bytes, row, tight) = (self.bytes, self.row, self.tight);
        (0..self.rows).map(move |r| &bytes[r * row..r * row + tight])
    }
}

/// Columns `0..w` of rows `y0..y0 + rows` of `texture` back from the GPU, a
/// band of about 64 MB at a time, each handed to `each` as it is mapped.
pub fn read_rows(
    gpu: &Gpu,
    texture: &wgpu::Texture,
    w: u32,
    y0: u32,
    rows: u32,
    mut each: impl FnMut(Band<'_>),
) -> std::result::Result<(), WaitError> {
    let device = &gpu.device;
    let texel_bytes = texture
        .format()
        .block_copy_size(None)
        .ok_or_else(|| WaitError::Failed("a texture with no copy size".into()))?;
    let tight = w * texel_bytes;
    let row =
        tight.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let step = band_rows(row as usize).min(rows).max(1);
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: row as u64 * step as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    for at in (y0..y0 + rows).step_by(step as usize) {
        let n = step.min(y0 + rows - at);
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x: 0, y: at, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(n),
                },
            },
            wgpu::Extent3d {
                width: w,
                height: n,
                depth_or_array_layers: 1,
            },
        );
        gpu.queue.submit(Some(encoder.finish()));
        let slice = buffer.slice(..row as u64 * n as u64);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        wait(device, None)?;
        rx.recv()
            .map_err(|e| WaitError::Failed(e.to_string()))?
            .map_err(|e| WaitError::Failed(e.to_string()))?;
        {
            let bytes = slice.get_mapped_range();
            each(Band {
                bytes: &bytes,
                row: row as usize,
                tight: tight as usize,
                rows: n as usize,
            });
        }
        buffer.unmap();
    }
    Ok(())
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

    const MB64: usize = 64 << 20;

    #[test]
    fn a_band_is_about_64_mb_and_never_empty() {
        // Well below the limit: many rows.
        assert_eq!(band_rows(1024), (MB64 / 1024) as u32);
        // At the limit: exactly one row.
        assert_eq!(band_rows(MB64), 1);
        // One byte under: still one row (integer division).
        assert_eq!(band_rows(MB64 - 1), 1);
        assert_eq!(band_rows(MB64 / 2), 2);
        // A row wider than the limit still moves, one row at a time.
        assert_eq!(band_rows(MB64 + 1), 1);
        assert_eq!(band_rows(usize::MAX), 1);
        // A degenerate row of no bytes does not divide by zero.
        assert_eq!(band_rows(0), (MB64) as u32);
    }

    #[test]
    fn a_band_hands_out_each_rows_texels_without_the_padding() {
        // 3 texels of 4 bytes = 12 tight bytes, rows 256 apart.
        let (tight, row, rows) = (12usize, 256usize, 3usize);
        assert_eq!(row % wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize, 0);
        let mut bytes = vec![0xEEu8; row * rows];
        for r in 0..rows {
            for i in 0..tight {
                bytes[r * row + i] = (r * 16 + i) as u8;
            }
        }
        let band = Band {
            bytes: &bytes,
            row,
            tight,
            rows,
        };
        let lines: Vec<&[u8]> = band.lines().collect();
        assert_eq!(lines.len(), rows);
        for (r, line) in lines.iter().enumerate() {
            let want: Vec<u8> = (0..tight).map(|i| (r * 16 + i) as u8).collect();
            assert_eq!(*line, &want[..]);
        }
    }

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
