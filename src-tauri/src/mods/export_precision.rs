//! High-precision export: 32-bit float through the render, 16-bit on disk.
//!
//! WHAT WAS WRONG
//!
//! A TIFF export wrote `ImageRgb16(image.to_rgb16())` over an RGBA8 buffer, so
//! every value was an 8-bit number multiplied by 257. The file said 16-bit, the
//! header said 16-bit, Photoshop said 16-bit, and the data was 8-bit. That is
//! worse than exporting 8-bit honestly, because nothing anywhere says otherwise.
//!
//! WHY 32-BIT AND NOT HALF
//!
//! RapidRAW PR #1466 solves the same problem with a half-float render target,
//! which is a large improvement and still not a 16-bit file: f16 carries 11
//! significant bits, so the brightest stop of an encoded signal lands on a grid
//! of about 32 steps out of 65535. It is invisible on screen and it is not what
//! the container promises.
//!
//! darktable carries 32-bit float from `rawprepare` to the format plugin and
//! quantises once, at the encode, with no dither. It offers no precision choice,
//! only a container depth. The render here follows that shape: one path, f32
//! through the compute pass, `u16` only when the TIFF is written.
//!
//! WHAT THIS DOES NOT DO, AND WHY IT IS SAID OUT LOUD
//!
//! The input is still uploaded as `Rgba16Float`, exactly as upstream does it, so
//! this is not f32 end to end. A 14-bit sensor holds 13 bits in its top stop and
//! half-float keeps 11, so the two brightest stops are thinned on the way in.
//!
//! An earlier version of this comment said that happened *before* highlight
//! recovery, which would have made it serious. It is the other way round:
//! `raw_processing.rs:77` calls `mods::decode::on_raw_decoded` the moment rawler
//! finishes, and `highlights::recover` runs there, on the Bayer data, long before
//! anything reaches a texture. Recovery sees the full sensor precision. What the
//! f16 upload costs is the *rendering* of those stops, not their reconstruction -
//! a real cost, and a smaller one. Checked, because the first version of this
//! paragraph was wrong and read as though it had been.
//!
//! It is left alone deliberately rather than overlooked. The flare pass binds the
//! input texture as filterable and reads it through a `Filtering` sampler
//! (`flare.wgsl`, `textureSampleLevel`), and a filtered `Rgba32Float` needs the
//! `FLOAT32_FILTERABLE` device feature, which is not requested and is not present
//! everywhere. Making the input f32 therefore means either a capability fork in
//! the render - the same export producing different precision on different
//! machines, which is the silent degradation this module exists to prevent - or
//! changing the format of a texture that every preview also uses. That is a
//! second feature with a different blast radius, and it is registered as one.
//!
//! So what this removes is the 8-bit *output* bottleneck, and only that. The
//! result is bounded by the f16 input and the f16 intermediates at about 11
//! significant bits rather than by the output at 8. "Quantises once" is true of
//! the path from the compute pass to the file; it is not true of the whole
//! pipeline, and saying otherwise would be the same kind of overclaim as the
//! 16-bit label this exists to make honest.
//!
//! WHAT CAME FROM UPSTREAM
//!
//! The idea of building a second pipeline by rewriting the storage format in the
//! shader source, and of gating the dither behind a pipeline constant, are from
//! dimafa's PR #1466 (`0e8cd15977001cee9f86d5efb6adccc105db4cb1`). The approach
//! is theirs and it is a good one. The precision, the composition with our own
//! shader modules, and the tests are ours.
//!
//! The *idea* is borrowed, not the text: their override is a `u32` tested with
//! `== 0u`, this is a `bool` and a negation. Same behaviour, different source. So
//! the borrow markers in `shader.wgsl` record where the thinking came from; they
//! do not promise git will merge their version without an argument.
//!
//! Those markers are written out only in the file that carries their code. Quoting
//! one here made this file look like it held a borrowed fix of its own, and the
//! integrity check said so - correctly - the moment a stricter version of it
//! arrived from main.
//!
//! No ICC profile is attached, here or anywhere else in the export: nothing in
//! this tree writes one for any format, and giving TIFF alone a profile would
//! make it the odd one out rather than the correct one. Tagging every export is
//! its own change.
//!
//! Nothing of PR #1395 is used: the bounded intermediate textures it adds are
//! already in this tree as `clamped_tile_size`, and its capability gate belongs
//! to the half-float design this did not follow. That gate has not been re-read
//! against a 32-bit target - if the f32 *input* on the roadmap is ever attempted,
//! it is the first thing to look at rather than the first thing to dismiss.

use image::{DynamicImage, GenericImageView, ImageBuffer, Rgba};
use wgpu::util::{DeviceExt, TextureDataOrder};

use crate::AppState;
use crate::gpu_processing::{GpuProcessor, RenderRequest, process_and_get_dynamic_image};
use crate::image_processing::GpuContext;

/// The shader the renderer actually compiles: our modules, then theirs.
///
/// Shared so the export variant is provably a transformation of the same text
/// rather than a second copy that can drift. Upstream's own version of this
/// reads `include_str!("shaders/shader.wgsl")` alone, which in this tree would
/// drop `modules.wgsl` and leave every `ag_` function undefined - every render
/// would fail, previews included, not just a 16-bit export.
pub const SHADER_SOURCE: &str = concat!(
    include_str!("../shaders/modules.wgsl"),
    include_str!("../shaders/shader.wgsl"),
);

/// What the storage texture is declared as in their shader.
const STORAGE_8: &str = "rgba8unorm, write>";

/// What it has to be for a high-precision render.
const STORAGE_32: &str = "rgba32float, write>";

/// Shader source for the export pipeline.
///
/// `Err` rather than a silent no-op when the declaration is not found exactly
/// once: if upstream respells it, the replace would quietly do nothing, the bind
/// group layout would say `Rgba32Float`, the shader would still say
/// `rgba8unorm`, and pipeline creation would fail at the moment a user first
/// exports. A test asserts this on every build instead.
pub fn export_shader_source() -> Result<String, String> {
    let hits = SHADER_SOURCE.matches(STORAGE_8).count();
    if hits != 1 {
        return Err(format!(
            "expected exactly one `{STORAGE_8}` in the shader, found {hits}. \
             Upstream has renamed the output storage texture; the export pipeline \
             must be updated rather than silently rendering at 8 bits."
        ));
    }
    Ok(SHADER_SOURCE.replacen(STORAGE_8, STORAGE_32, 1))
}

/// Name of the pipeline constant that turns the dither off.
///
/// Their shader adds +/-0.5/255 of noise before the store, which is right for an
/// 8-bit target and 257 times too large for a 16-bit one. Taken from PR #1466.
pub const DITHER_OVERRIDE: &str = "HIGH_PRECISION_OUTPUT";

/// Bytes per pixel of the high-precision render target.
pub const BYTES_PER_PIXEL: u32 = 16;

/// One `f32` sample to one `u16`, the way darktable does it: clamp, scale, round.
///
/// No dither. The shader has already done its arithmetic in f32 and the only
/// quantisation left is this one, whose error is at most half of 1/65535 - some
/// 90 dB below any sensor's noise floor. Dithering it would add noise to hide a
/// step nobody can measure.
///
/// This is a *specification*, not the code that runs. The encoder is
/// `DynamicImage::to_rgb16`, and what matters is that the two agree - so this is
/// written the same way round as the `image` crate's `normalize_float` and a test
/// compares them sample for sample. Note the comparison is `!(v < 1.0)` rather
/// than `v.clamp(0.0, 1.0)`: clamp propagates NaN, and the encoder maps NaN to
/// white. Two different answers for a NaN is exactly the sort of disagreement
/// that would only ever show up in somebody's exported file. The `image` crate
/// spells it `!(float < 1.0)`; this spells the same three cases out, because
/// clippy is right that a negated comparison on a partially ordered type is hard
/// to read - and the NaN case is the whole point of the line.
/// Test-only, and that is the point: if it ever acquires a caller, the encode has
/// stopped being upstream's and the parity test has stopped meaning anything.
#[cfg(test)]
#[inline]
pub fn sample_to_u16(v: f32) -> u16 {
    let clamped = if v.is_nan() || v >= 1.0 {
        1.0
    } else {
        v.max(0.0)
    };
    (clamped * 65535.0).round() as u16
}

/// What bit depth a TIFF is written at.
///
/// A choice, because the two answers are for different jobs rather than one
/// being better: 8 bits is a delivery, 16 is a master somebody will edit again.
/// Every other format here is 8-bit by its own definition, so this applies to
/// TIFF and nothing else.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TiffDepth {
    /// The same precision a JPEG gets, in a TIFF container. Smaller file.
    Eight,
    /// What Argentum exported before there was a choice, and still the default.
    Sixteen,
}

impl TiffDepth {
    /// How the frontend names it, and how it is stored.
    pub fn as_u8(self) -> u8 {
        match self {
            TiffDepth::Eight => 8,
            TiffDepth::Sixteen => 16,
        }
    }

    /// Anything that is not a depth we support is 16, which is what an export
    /// did before the setting existed. A preferences file written by a newer
    /// Argentum must not make an older one export at a depth it cannot render.
    pub fn from_u8(value: u8) -> Self {
        match value {
            8 => TiffDepth::Eight,
            _ => TiffDepth::Sixteen,
        }
    }
}

/// The chosen depth, read on every export and written when the user picks one.
///
/// An atomic rather than a lock: it is read on a render thread and written from
/// the UI, and it is one byte.
static TIFF_DEPTH: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(16);

/// The key in `argentum-processing.json`.
const TIFF_DEPTH_KEY: &str = "tiffBitDepth";

/// What an export will use right now.
pub fn tiff_depth() -> TiffDepth {
    TiffDepth::from_u8(TIFF_DEPTH.load(std::sync::atomic::Ordering::Relaxed))
}

/// Read the preference at startup. Missing or unreadable means 16-bit, which is
/// what every export did before this setting existed - so upgrading changes
/// nobody's output until they ask for it.
pub fn load(library: &std::path::Path) {
    let depth = crate::mods::ag_settings::get(library, TIFF_DEPTH_KEY)
        .and_then(|v| v.as_u64())
        .map(|v| TiffDepth::from_u8(v as u8))
        .unwrap_or(TiffDepth::Sixteen);
    TIFF_DEPTH.store(depth.as_u8(), std::sync::atomic::Ordering::Relaxed);
}

/// Write it, and apply it now.
pub fn save(library: &std::path::Path, depth: TiffDepth) -> Result<(), String> {
    TIFF_DEPTH.store(depth.as_u8(), std::sync::atomic::Ordering::Relaxed);
    crate::mods::ag_settings::set(
        library,
        TIFF_DEPTH_KEY,
        serde_json::Value::from(depth.as_u8()),
    )
}

/// Write a TIFF at the depth its pixels are already carrying.
///
/// BORROWED, and deliberately not the way they wrote it.
///
/// The encoder arm is upstream #1466's: 8-bit images are written as `Rgb8`,
/// everything else as `Rgb16`, and the match covers `"tif"` as well as
/// `"tiff"` - a gap that has always been here, where exporting to a `.tif` path
/// failed with "Unsupported file format".
///
/// What is not borrowed is how the decision arrives. Theirs adds a fourth
/// parameter to `encode_image_to_bytes` and threads it through six call sites in
/// their files, which is the exact pattern `CLAUDE.md` names as the thing that
/// kills a fork. Here the depth is already in the image: an 8-bit export renders
/// to `ImageRgba8` and a 16-bit one to `ImageRgba32F`, so the encoder reads what
/// it was handed and their file keeps one call.
pub fn encode_tiff<W: std::io::Write + std::io::Seek>(
    image: &DynamicImage,
    into: &mut W,
) -> Result<(), String> {
    let to_encode = match image {
        DynamicImage::ImageRgb8(_) | DynamicImage::ImageRgba8(_) => {
            DynamicImage::ImageRgb8(image.to_rgb8())
        }
        _ => DynamicImage::ImageRgb16(image.to_rgb16()),
    };
    to_encode
        .write_to(into, image::ImageFormat::Tiff)
        .map_err(|e| e.to_string())
}

/// Which render target a pipeline is built for.
///
/// Everything that differs between the preview render and the export render is
/// reachable from this one value: the storage format, the shader text, the
/// pipeline constant that silences the dither, and how many bytes a pixel takes
/// coming back. `gpu_processing.rs` imports this type and nothing else of ours,
/// so the whole feature costs their file a single line that mentions Argentum.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Precision {
    /// Upstream's own path, byte for byte: 8-bit storage, dither on.
    Preview,
    /// 32-bit float storage, dither off. Built for an export and dropped after.
    High,
}

impl Precision {
    /// The format of the texture the compute pass writes into.
    ///
    /// Only the *tile* texture changes. `working_texture` and `output_texture`
    /// are written by a copy that runs solely when `output_to_display` is set,
    /// which an export never sets, so they keep upstream's `Rgba8Unorm`.
    pub fn storage_format(self) -> wgpu::TextureFormat {
        match self {
            Precision::Preview => wgpu::TextureFormat::Rgba8Unorm,
            Precision::High => wgpu::TextureFormat::Rgba32Float,
        }
    }

    /// How big to make the two textures an export never writes to.
    ///
    /// `working_texture` and `output_texture` exist for the display path and the
    /// asynchronous analytics readback. Both are behind `output_to_display`,
    /// which an export does not set, so for an export they are allocated,
    /// touched by nothing, and freed.
    ///
    /// At full resolution that is not free. An 11648x8736 frame is 407 MB each,
    /// so an export was quietly asking for 814 MB of nothing on top of the
    /// preview processor that is already resident - the first review of this
    /// feature caught a comment here claiming the whole cost was "about 50 MB",
    /// which was true of the tile and false of the export. One pixel each
    /// instead. If upstream ever makes that copy unconditional the destination
    /// is too small and wgpu rejects the copy, which is a loud failure rather
    /// than a torn image.
    pub fn scratch_size(self, full: wgpu::Extent3d) -> wgpu::Extent3d {
        match self {
            Precision::Preview => full,
            Precision::High => wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        }
    }

    /// Bytes per pixel coming back from that texture.
    pub fn bytes_per_pixel(self) -> u32 {
        match self {
            Precision::Preview => 4,
            Precision::High => BYTES_PER_PIXEL,
        }
    }

    /// The shader text to compile.
    pub fn shader_source(self) -> Result<std::borrow::Cow<'static, str>, String> {
        match self {
            Precision::Preview => Ok(std::borrow::Cow::Borrowed(SHADER_SOURCE)),
            Precision::High => export_shader_source().map(std::borrow::Cow::Owned),
        }
    }

    /// Pipeline constants, which is where the dither is turned off.
    pub fn shader_constants(self) -> &'static [(&'static str, f64)] {
        match self {
            Precision::Preview => &[],
            Precision::High => &[(DITHER_OVERRIDE, 1.0)],
        }
    }

    /// Which precision an export to this file should render at.
    ///
    /// The policy lives here rather than in the export code so that adding a
    /// format, or a third precision, costs nothing in a file upstream owns. TIFF
    /// only: it is the one container in the list that carries more than eight
    /// bits per channel. JPEG and WebP are 8-bit formats, and PNG's branch in
    /// `encode_image_to_bytes` only widens for `Rgb32F`, so routing it here would
    /// change what that branch does.
    ///
    /// A TIFF the user has asked for at 8 bits renders on the preview path, which
    /// is the same render a JPEG export gets - dither included, because at 8 bits
    /// the dither is right.
    pub fn for_path(path: &std::path::Path) -> Self {
        Self::for_extension(path.extension().and_then(|e| e.to_str()).unwrap_or(""))
    }

    /// The same decision, from a bare extension.
    ///
    /// The per-mask export builds its filenames from a string rather than a path,
    /// and it was missed on the first pass: with "export masks" on, the main TIFF
    /// took the high-precision path while every `_mask_N_image.tiff` written
    /// beside it still carried the exact bug this feature exists to fix.
    pub fn for_extension(extension: &str) -> Self {
        Self::for_extension_at(extension, tiff_depth())
    }

    /// The same decision with the depth passed in.
    ///
    /// Split out so the policy can be tested without writing to the global the
    /// user's preference lives in - tests run in parallel, and one of them
    /// setting a depth while another reads it is a flake that would show up
    /// once a month and never reproduce.
    pub fn for_extension_at(extension: &str, depth: TiffDepth) -> Self {
        match extension.to_ascii_lowercase().as_str() {
            "tif" | "tiff" => match depth {
                TiffDepth::Sixteen => Precision::High,
                TiffDepth::Eight => Precision::Preview,
            },
            _ => Precision::Preview,
        }
    }
}

/// Render for export at full precision, on a processor that exists only for this
/// call.
///
/// Deliberately not routed through `process_and_get_dynamic_image`: that function
/// keeps its processor and its input texture in `AppState` so consecutive
/// previews reuse them. An export borrowing that cache would leave a
/// full-resolution, 32-bit-target processor behind for every later preview to
/// pay for. This builds one, uses it, and drops it at the end of the function.
/// One export render at a time.
///
/// `process_and_get_dynamic_image_inner` holds `state.gpu_processor` for the
/// whole render, so it serialised GPU work as a side effect of caching the
/// processor. This path deliberately does not take that lock - it builds its own
/// processor so a preview never inherits a 32-bit target - and without this it
/// would have removed the serialisation with it: the batch export scheduler
/// sizes its worker pool from system RAM, not VRAM, so several workers would
/// have held a processor and a float readback at once on hardware that was
/// previously only ever asked for one.
static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub fn render_high_precision(
    context: &GpuContext,
    base_image: &DynamicImage,
    request: RenderRequest,
) -> Result<DynamicImage, String> {
    let _serialised = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let (width, height) = base_image.dimensions();
    let max_dim = context.limits.max_texture_dimension_2d;
    if width > max_dim || height > max_dim {
        return Err(format!(
            "{width}x{height} exceeds this GPU's maximum texture dimension of {max_dim}"
        ));
    }

    // f16 on the way in, as upstream uploads it. See the note at the top of this
    // file: the flare pass samples this texture through a filtering sampler, and
    // a filtered Rgba32Float needs a device feature we do not require.
    let texels = crate::gpu_processing::to_rgba_f16(base_image);
    let input_texture = context.device.create_texture_with_data(
        &context.queue,
        &wgpu::TextureDescriptor {
            label: Some("High Precision Export Input"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        },
        TextureDataOrder::MipMajor,
        bytemuck::cast_slice(&texels),
    );
    let input_view = input_texture.create_view(&Default::default());

    let processor = GpuProcessor::new_with_precision(
        context.clone(),
        (width + 255) & !255,
        (height + 255) & !255,
        Precision::High,
    )?;

    let (bytes, out_w, out_h, _, _) =
        processor.run(&input_view, width, height, request, false, false)?;

    let expected = out_w as usize * out_h as usize * BYTES_PER_PIXEL as usize;
    if bytes.len() != expected {
        return Err(format!(
            "high-precision readback was {} bytes, expected {expected} for {out_w}x{out_h}",
            bytes.len()
        ));
    }

    // Not `bytemuck::cast_slice`: that panics on a buffer whose address is not
    // 4-aligned, and nothing in the type of a `Vec<u8>` from a mapped GPU buffer
    // promises alignment. It happens to hold with this allocator, which is the
    // sort of thing that holds until it does not.
    let samples = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_ne_bytes(*b))
        .collect();
    pixels_to_image(out_w, out_h, samples)
}

/// Composite a watermark without dragging the photograph through 8 bits.
///
/// `image::imageops::overlay` works through `GenericImage for DynamicImage`,
/// whose pixel type is `Rgba<u8>`. On a 32-bit float image it therefore reads,
/// blends and writes back every pixel inside the watermark's bounding rectangle
/// at 8 bits - including the fully transparent ones, which are quantised for
/// nothing at all. On a file that says 16-bit that is a rectangle of the
/// photograph quietly demoted, and at opacity zero it is pure loss with no
/// watermark to show for it.
///
/// The arithmetic is theirs, taken from `Blend for Rgba<T>` so that an 8-bit
/// export is unchanged: premultiply, source-over, unmultiply. Only the precision
/// and the transparent-pixel short circuit differ.
pub fn overlay_preserving_precision(base: &mut DynamicImage, top: &DynamicImage, x: i64, y: i64) {
    let Some(canvas) = base.as_mut_rgba32f() else {
        // Not a high-precision render: theirs, untouched, byte for byte.
        image::imageops::overlay(base, top, x, y);
        return;
    };

    let (width, height) = (canvas.width(), canvas.height());
    let stamp = top.to_rgba8();

    for (sx, sy, px) in stamp.enumerate_pixels() {
        let fg_a = px[3] as f32 / 255.0;
        if fg_a <= 0.0 {
            // The pixel `overlay` would have rewritten for no reason.
            continue;
        }

        let (Ok(bx), Ok(by)) = (u32::try_from(x + sx as i64), u32::try_from(y + sy as i64)) else {
            continue;
        };
        if bx >= width || by >= height {
            continue;
        }

        let dst = canvas.get_pixel_mut(bx, by);
        let bg_a = dst[3];
        let out_a = bg_a + fg_a - bg_a * fg_a;
        if out_a == 0.0 {
            continue;
        }
        for c in 0..3 {
            let fg = px[c] as f32 / 255.0;
            dst[c] = (fg * fg_a + dst[c] * bg_a * (1.0 - fg_a)) / out_a;
        }
        dst[3] = out_a;
    }
}

/// The one entry point the export code calls, whichever precision it wants.
///
/// The branch lives here rather than in `export_processing.rs` so that their file
/// carries a call and not a decision: adding a format, or a third precision,
/// changes this function and nothing of theirs.
pub fn render_for_export(
    context: &GpuContext,
    state: &tauri::State<AppState>,
    base_image: &DynamicImage,
    transform_hash: u64,
    request: RenderRequest,
    debug_tag: &str,
    precision: Precision,
) -> Result<DynamicImage, String> {
    match precision {
        Precision::High => render_high_precision(context, base_image, request),
        Precision::Preview => process_and_get_dynamic_image(
            context,
            state,
            base_image,
            transform_hash,
            request,
            debug_tag,
        ),
    }
}

/// A finished f32 readback to an image, without passing through 8 bits.
pub fn pixels_to_image(width: u32, height: u32, pixels: Vec<f32>) -> Result<DynamicImage, String> {
    let expected = width as usize * height as usize * 4;
    if pixels.len() != expected {
        return Err(format!(
            "high-precision readback was {} samples, expected {expected} for {width}x{height}",
            pixels.len()
        ));
    }
    ImageBuffer::<Rgba<f32>, Vec<f32>>::from_raw(width, height, pixels)
        .map(DynamicImage::ImageRgba32F)
        .ok_or_else(|| "could not build a 32-bit image from the readback".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shader_we_compile_carries_our_modules() {
        assert!(
            SHADER_SOURCE.contains("fn ag_to_scene_linear"),
            "modules.wgsl is missing from the composed shader - the export \
             pipeline would compile a shader with every ag_ function undefined",
        );
    }

    #[test]
    fn the_export_variant_swaps_the_storage_format_exactly_once() {
        let src = export_shader_source().expect("the declaration should be found");
        assert_eq!(src.matches(STORAGE_32).count(), 1);
        assert_eq!(
            src.matches(STORAGE_8).count(),
            0,
            "an 8-bit storage declaration survived into the high-precision shader",
        );
        assert!(
            src.contains("fn ag_to_scene_linear"),
            "our modules were lost"
        );
    }

    /// The guard that matters: if upstream renames the declaration, fail loudly
    /// rather than render at 8 bits and call the file 16-bit.
    #[test]
    fn a_missing_declaration_is_an_error_not_a_silent_no_op() {
        let hits = SHADER_SOURCE.matches(STORAGE_8).count();
        assert_eq!(
            hits, 1,
            "shader.wgsl no longer declares `{STORAGE_8}` exactly once. \
             export_shader_source() is now returning Err, which is correct, but \
             the export pipeline needs updating to whatever replaced it.",
        );
    }

    #[test]
    fn quantisation_is_clamp_scale_round() {
        assert_eq!(sample_to_u16(0.0), 0);
        assert_eq!(sample_to_u16(1.0), 65535);
        assert_eq!(sample_to_u16(-0.5), 0, "below black clamps, not wraps");
        assert_eq!(sample_to_u16(2.5), 65535, "above white clamps, not wraps");
        assert_eq!(sample_to_u16(0.5), 32768, "rounds rather than truncating");
        // The step an 8-bit pipeline cannot express: 1/65535 apart, distinct.
        assert_ne!(sample_to_u16(0.500_01), sample_to_u16(0.500_03));
    }

    /// The negative control this whole feature exists to fail.
    ///
    /// An 8-bit value widened to 16 bits is always a multiple of 257. Real
    /// high-precision output is not. A test that cannot tell those apart would
    /// pass on the exact bug being fixed.
    #[test]
    fn expanded_eight_bit_is_detectable_and_our_output_is_not_it() {
        let expanded: Vec<u16> = (0..=255u16).map(|v| v * 257).collect();
        assert!(
            expanded.iter().all(|v| v % 257 == 0),
            "the control itself is wrong",
        );

        // A gradient finer than 8 bits can express.
        let ours: Vec<u16> = (0..1024)
            .map(|i| sample_to_u16(i as f32 / 1023.0))
            .collect();
        let off_lattice = ours.iter().filter(|v| *v % 257 != 0).count();
        assert!(
            off_lattice > 900,
            "only {off_lattice} of 1024 samples were off the 8-bit lattice; \
             this output is 8-bit data wearing a 16-bit label",
        );
    }

    /// And the control for half-float, which is the other bottleneck.
    ///
    /// The ramp has to be finer than f16's own grid or the test measures
    /// nothing. In [0.5, 1) f16 steps by 2^-11, so a ramp spanning that whole
    /// stop in 512 samples steps by *more* than f16 does and every value
    /// survives intact. The first version of this test did exactly that, and
    /// reported half-float as indistinguishable from f32. This one walks 0.02
    /// in 512 steps - roughly twelve samples per f16 step - so anything
    /// quantised to half collapses and anything carrying f32 does not.
    #[test]
    fn half_float_rounding_is_detectable_and_our_output_is_not_it() {
        let at = |i: i32| 0.5 + (i as f32 / 511.0) * 0.02;

        let via_f16: Vec<u16> = (0..512)
            .map(|i| sample_to_u16(half::f16::from_f32(at(i)).to_f32()))
            .collect();
        let distinct_f16 = via_f16
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len();

        let direct: Vec<u16> = (0..512).map(|i| sample_to_u16(at(i))).collect();
        let distinct_f32 = direct
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len();

        assert!(
            distinct_f16 < 100,
            "half-float should collapse this ramp onto a coarse grid, got {distinct_f16} \
             distinct values - the control is not measuring what it claims",
        );
        assert!(
            distinct_f32 > distinct_f16 * 2,
            "f32 gave {distinct_f32} distinct values against f16's {distinct_f16}; \
             the render is not carrying more than half-float precision",
        );
    }

    /// A transparent watermark must leave the photograph alone.
    ///
    /// `imageops::overlay` rewrites every pixel in the stamp's bounding box
    /// whatever its alpha, so at opacity zero it quantised a rectangle of the
    /// picture to 8 bits and put nothing there. This is the regression test for
    /// that, and it fails against the old code rather than merely describing it.
    #[test]
    fn a_fully_transparent_watermark_changes_nothing() {
        let mut buf = ImageBuffer::<Rgba<f32>, Vec<f32>>::new(8, 4);
        for (i, px) in buf.pixels_mut().enumerate() {
            let v = 0.5 + i as f32 * 0.000_01;
            *px = Rgba([v, v * 0.9, v * 0.8, 1.0]);
        }
        let before = buf.clone();
        let mut base = DynamicImage::ImageRgba32F(buf);

        let clear =
            DynamicImage::ImageRgba8(ImageBuffer::from_pixel(8, 4, image::Rgba([255u8, 0, 0, 0])));
        overlay_preserving_precision(&mut base, &clear, 0, 0);

        assert_eq!(
            base.as_rgba32f().expect("still float").as_raw(),
            before.as_raw(),
            "a watermark at zero opacity altered the image underneath it",
        );
    }

    /// And where it is not transparent, the blend itself keeps precision.
    #[test]
    fn a_blended_watermark_does_not_land_on_the_eight_bit_lattice() {
        let mut buf = ImageBuffer::<Rgba<f32>, Vec<f32>>::new(64, 1);
        for (i, px) in buf.pixels_mut().enumerate() {
            let v = 0.25 + i as f32 * 0.000_05;
            *px = Rgba([v, v, v, 1.0]);
        }
        let mut base = DynamicImage::ImageRgba32F(buf);

        let stamp = DynamicImage::ImageRgba8(ImageBuffer::from_pixel(
            64,
            1,
            image::Rgba([255u8, 255, 255, 64]),
        ));
        overlay_preserving_precision(&mut base, &stamp, 0, 0);

        let out = base.to_rgb16();
        let row: Vec<u16> = out.pixels().map(|p| p[0]).collect();
        let distinct = row.iter().collect::<std::collections::HashSet<_>>().len();
        assert!(
            distinct > 40,
            "64 distinguishable inputs blended down to {distinct} values under the \
             watermark - the composite is going through 8 bits",
        );
        assert!(
            row.iter().any(|v| v % 257 != 0),
            "every blended sample landed on the 8-bit lattice",
        );
    }

    /// An 8-bit export must be byte for byte what it was.
    #[test]
    fn an_eight_bit_image_still_gets_upstreams_own_overlay() {
        let base_pixels = ImageBuffer::from_fn(6, 3, |x, y| {
            image::Rgba([(x * 40) as u8, (y * 80) as u8, 30u8, 255u8])
        });
        let stamp = DynamicImage::ImageRgba8(ImageBuffer::from_pixel(
            3,
            2,
            image::Rgba([10u8, 200, 90, 128]),
        ));

        let mut ours = DynamicImage::ImageRgba8(base_pixels.clone());
        overlay_preserving_precision(&mut ours, &stamp, 1, 1);

        let mut theirs = DynamicImage::ImageRgba8(base_pixels);
        image::imageops::overlay(&mut theirs, &stamp, 1, 1);

        assert_eq!(
            ours.to_rgba8().as_raw(),
            theirs.to_rgba8().as_raw(),
            "the 8-bit path diverged from upstream's overlay",
        );
    }

    #[test]
    fn an_unknown_depth_is_sixteen_rather_than_a_failure() {
        assert_eq!(TiffDepth::from_u8(8), TiffDepth::Eight);
        assert_eq!(TiffDepth::from_u8(16), TiffDepth::Sixteen);
        // A preferences file written by a newer Argentum, or a hand-edited one.
        for odd in [0u8, 1, 12, 24, 32, 255] {
            assert_eq!(
                TiffDepth::from_u8(odd),
                TiffDepth::Sixteen,
                "{odd} should fall back to what an export did before the setting                  existed, not to something nobody chose",
            );
        }
    }

    #[test]
    fn the_chosen_depth_decides_which_pipeline_runs() {
        assert_eq!(
            Precision::for_extension_at("tiff", TiffDepth::Sixteen),
            Precision::High,
        );
        assert_eq!(
            Precision::for_extension_at("tiff", TiffDepth::Eight),
            Precision::Preview,
            "an 8-bit TIFF should take the ordinary render, dither and all",
        );
        // The depth is a TIFF setting and must not leak into other formats.
        for ext in ["jpg", "png", "webp", "avif", "jxl"] {
            assert_eq!(
                Precision::for_extension_at(ext, TiffDepth::Sixteen),
                Precision::Preview,
                "{ext} is an 8-bit format and the TIFF depth must not touch it",
            );
        }
    }

    /// The borrowed encoder arm: the depth of the file follows the depth of the
    /// pixels it was handed, which is what lets their file keep one call.
    #[test]
    fn the_encoder_writes_the_depth_the_image_carries() {
        let eight = DynamicImage::ImageRgba8(ImageBuffer::from_fn(4, 2, |x, y| {
            image::Rgba([(x * 60) as u8, (y * 90) as u8, 20, 255])
        }));
        let mut bytes = Vec::new();
        encode_tiff(&eight, &mut std::io::Cursor::new(&mut bytes)).expect("encode 8-bit");
        let back = image::load_from_memory_with_format(&bytes, image::ImageFormat::Tiff)
            .expect("decode 8-bit");
        assert!(
            back.as_rgb8().is_some(),
            "an 8-bit image should produce an 8-bit TIFF, not a widened one",
        );
        assert_eq!(back.to_rgb8(), eight.to_rgb8(), "the pixels changed");

        let deep = ramp(4);
        let mut bytes16 = Vec::new();
        encode_tiff(&deep, &mut std::io::Cursor::new(&mut bytes16)).expect("encode 16-bit");
        let back16 = image::load_from_memory_with_format(&bytes16, image::ImageFormat::Tiff)
            .expect("decode 16-bit");
        assert!(
            back16.as_rgb16().is_some(),
            "a float image should still produce a 16-bit TIFF",
        );
        assert!(
            back16.to_rgb16().pixels().any(|p| p[0] % 257 != 0),
            "the 16-bit branch produced 8-bit data",
        );
    }

    /// An 8-bit TIFF is smaller than a 16-bit one, which is the reason to offer
    /// it. If this ever stops being true the option is pointless.
    #[test]
    fn eight_bit_is_the_smaller_file() {
        let deep = ramp(256);
        let flat = DynamicImage::ImageRgba8(deep.to_rgba8());

        let mut small = Vec::new();
        encode_tiff(&flat, &mut std::io::Cursor::new(&mut small)).expect("8-bit");
        let mut large = Vec::new();
        encode_tiff(&deep, &mut std::io::Cursor::new(&mut large)).expect("16-bit");

        assert!(
            small.len() < large.len(),
            "8-bit TIFF was {} bytes against 16-bit's {}",
            small.len(),
            large.len(),
        );
    }

    /// The policy, locked down, including the spelling the mask export uses.
    ///
    /// `for_path` and `for_extension` have to agree: the main export asks with a
    /// path, the per-mask export asks with a bare string, and when they disagreed
    /// the result was one 16-bit file and N 8-bit companions beside it.
    #[test]
    fn every_caller_gets_the_same_answer_for_the_same_format() {
        for ext in ["tiff", "TIFF", "tif", "Tif"] {
            assert_eq!(Precision::for_extension(ext), Precision::High, "{ext}");
            assert_eq!(
                Precision::for_path(std::path::Path::new(&format!("a/b.{ext}"))),
                Precision::High,
                "{ext} through a path",
            );
        }
        for ext in ["jpg", "jpeg", "png", "webp", "avif", "jxl", ""] {
            assert_eq!(Precision::for_extension(ext), Precision::Preview, "{ext}");
        }
        assert_eq!(
            Precision::for_path(std::path::Path::new("no-extension")),
            Precision::Preview,
        );
    }

    #[test]
    fn readback_length_is_checked_rather_than_trusted() {
        assert!(pixels_to_image(2, 2, vec![0.0; 16]).is_ok());
        assert!(
            pixels_to_image(2, 2, vec![0.0; 12]).is_err(),
            "a short readback must be an error, not a panic or a torn image",
        );
    }

    fn ramp(n: u32) -> DynamicImage {
        let mut buf = ImageBuffer::<Rgba<f32>, Vec<f32>>::new(n, 1);
        for (i, px) in buf.pixels_mut().enumerate() {
            let v = 0.5 + i as f32 * 0.000_05;
            *px = Rgba([v, v, v, 1.0]);
        }
        DynamicImage::ImageRgba32F(buf)
    }

    /// The encoder, not our copy of it.
    ///
    /// `encode_image_to_bytes` writes a TIFF with `DynamicImage::to_rgb16()`, so
    /// that is the function whose behaviour decides what lands on disk. It was
    /// left exactly as upstream wrote it - it was never the broken part; it was
    /// being handed 8-bit data. This asserts it does the right thing with f32
    /// data, which is the only reason it is safe to leave alone.
    #[test]
    fn the_encoder_keeps_precision_from_a_float_image() {
        let out = ramp(4).to_rgb16();
        let reds: Vec<u16> = out.pixels().map(|p| p[0]).collect();
        assert_eq!(
            reds.iter().collect::<std::collections::HashSet<_>>().len(),
            4,
            "four inputs a 16-bit file can distinguish collapsed into fewer values -              to_rgb16 is routing f32 through 8 bits",
        );
        assert!(
            reds.iter().any(|v| v % 257 != 0),
            "output landed entirely on the 8-bit lattice",
        );
    }

    /// And that it agrees with the specification above, so the doc comment is not
    /// describing a function nobody calls.
    ///
    /// If the `image` crate ever changes how it quantises - including what it does
    /// with a NaN - this fails and the encode moves to our own implementation.
    #[test]
    fn the_encoder_quantises_the_way_this_module_says_it_does() {
        let probes: Vec<f32> = (0..2048)
            .map(|i| -0.1 + i as f32 * (1.2 / 2047.0))
            .chain([f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -0.0])
            .collect();

        let mut buf = ImageBuffer::<Rgba<f32>, Vec<f32>>::new(probes.len() as u32, 1);
        for (px, v) in buf.pixels_mut().zip(&probes) {
            *px = Rgba([*v, *v, *v, 1.0]);
        }
        let encoded = DynamicImage::ImageRgba32F(buf).to_rgb16();

        for (px, v) in encoded.pixels().zip(&probes) {
            assert_eq!(
                px[0],
                sample_to_u16(*v),
                "the encoder and sample_to_u16 disagree on {v}",
            );
        }
    }
}

/// The test that actually renders.
///
/// Everything above this point is arithmetic and text: it proves the shader
/// parses, that the storage declaration is rewritten, that the quantisation is
/// right. None of it proves a GPU will accept an `rgba32float` storage texture
/// with this bind group layout and hand back sixteen bytes a pixel - and that is
/// the step where a wrong answer looks like a correct one, because a torn or
/// mis-strided readback still produces a picture.
///
/// So this builds a real device, renders a ramp finer than eight bits can carry,
/// and checks the values that come back are off the 8-bit lattice. It is
/// `#[ignore]`d: CI has no adapter, and a test that fails for want of a GPU
/// teaches nobody anything. Run it on a machine that has one:
///
/// ```bash
/// cargo test --lib -- --ignored gpu
/// ```
#[cfg(test)]
mod gpu_tests {
    use super::*;
    use crate::image_processing::get_all_adjustments_from_json;

    /// A genuinely neutral edit.
    ///
    /// NOT `AllAdjustments::default()`. Every field of that is zero, including
    /// the camera-profile matrix - and `ag_camera_profile` in `modules.wgsl`
    /// multiplies by it, so a zeroed one turns every pixel black. The first run
    /// of the test below did exactly that and reported 512 inputs collapsing to
    /// one value, which reads like a precision failure and was a black frame.
    /// This is the same function the export itself calls, so the test exercises
    /// the adjustments the app really builds.
    fn neutral() -> crate::image_processing::AllAdjustments {
        get_all_adjustments_from_json(&serde_json::json!({}), false, None, None)
    }

    fn device() -> Option<GpuContext> {
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }))
        .ok()?;
        let limits = adapter.limits();
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("export precision test"),
            required_features: wgpu::Features::empty(),
            required_limits: limits.clone(),
            experimental_features: wgpu::ExperimentalFeatures::default(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        }))
        .ok()?;
        Some(GpuContext {
            device: std::sync::Arc::new(device),
            queue: std::sync::Arc::new(queue),
            limits,
            display: std::sync::Arc::new(std::sync::Mutex::new(None)),
        })
    }

    /// A horizontal ramp whose steps are finer than 1/255, so anything that
    /// passes through eight bits collapses them.
    fn fine_ramp(width: u32, height: u32) -> DynamicImage {
        let mut buf = ImageBuffer::<Rgba<f32>, Vec<f32>>::new(width, height);
        for (x, _y, px) in buf.enumerate_pixels_mut() {
            let v = 0.25 + (x as f32 / width as f32) * 0.25;
            *px = Rgba([v, v, v, 1.0]);
        }
        DynamicImage::ImageRgba32F(buf)
    }

    #[test]
    #[ignore = "needs a GPU; run with --ignored"]
    fn a_real_render_comes_back_at_more_than_eight_bits() {
        let Some(context) = device() else {
            panic!("no wgpu adapter - this test cannot tell you anything on this machine");
        };

        let base = fine_ramp(512, 8);
        let request = RenderRequest {
            adjustments: neutral(),
            mask_bitmaps: &[],
            lut: None,
            roi: None,
        };

        let out = render_high_precision(&context, &base, request)
            .expect("the high-precision render should succeed");

        let f32_image = out
            .as_rgba32f()
            .expect("the render returned something that is not 32-bit float");
        assert_eq!(f32_image.dimensions(), (512, 8));

        let encoded = out.to_rgb16();
        let row: Vec<u16> = (0..512).map(|x| encoded.get_pixel(x, 0)[0]).collect();

        let distinct = row.iter().collect::<std::collections::HashSet<_>>().len();
        let off_lattice = row.iter().filter(|v| *v % 257 != 0).count();

        assert!(
            distinct > 1,
            "the whole row came back as {}, which is a black or blown frame rather              than a precision problem - check the adjustments before the pipeline",
            row[0],
        );
        assert!(
            distinct > 200,
            "512 distinct inputs came back as {distinct} distinct outputs - the render \
             is quantising somewhere it should not",
        );
        assert!(
            off_lattice > 400,
            "only {off_lattice} of 512 samples were off the 8-bit lattice - this is \
             8-bit data in a 16-bit container, which is the bug this feature fixes",
        );

        // Monotonic, because the input is. A torn or mis-strided readback shows
        // up here as a sawtooth even when the histogram above looks healthy.
        let mut falls = 0;
        for pair in row.windows(2) {
            if pair[1] < pair[0] {
                falls += 1;
            }
        }
        assert!(
            falls < 8,
            "the output ramp reverses {falls} times across the row - the readback \
             strides are wrong",
        );

        // Every row of the image is the same ramp. If the row stride is wrong the
        // rows disagree, which the single-row checks above would never notice.
        for y in 1..8u32 {
            let other: Vec<u16> = (0..512).map(|x| encoded.get_pixel(x, y)[0]).collect();
            assert_eq!(
                other, row,
                "row {y} differs from row 0, and the input rows are identical - \
                 the readback row stride is wrong",
            );
        }
    }

    /// The one that exercises the strides: an odd size across a tile boundary,
    /// written to a real TIFF and read back.
    ///
    /// `run` tiles at 2048 with a 128 overlap and reassembles the readback row by
    /// row, and those row offsets are exactly the `* 4` -> `* bpp` lines this
    /// feature changed. A single-tile render never touches that arithmetic, so
    /// the test above could pass with the reassembly completely wrong. This one
    /// is 2501 wide - two tiles, neither a round number - and 97 tall, and it
    /// goes out through the same `ImageRgb16(to_rgb16())` call the exporter makes
    /// and comes back through the decoder.
    #[test]
    #[ignore = "needs a GPU; run with --ignored"]
    fn an_odd_size_across_a_tile_boundary_survives_a_tiff_round_trip() {
        let Some(context) = device() else {
            panic!("no wgpu adapter - this test cannot tell you anything on this machine");
        };

        const W: u32 = 2501;
        const H: u32 = 97;

        // A ramp along x, and a different offset per row, so a row that lands in
        // the wrong place is visible rather than merely suspicious.
        let mut buf = ImageBuffer::<Rgba<f32>, Vec<f32>>::new(W, H);
        for (x, y, px) in buf.enumerate_pixels_mut() {
            let v = 0.2 + (x as f32 / W as f32) * 0.3 + (y as f32 / H as f32) * 0.05;
            *px = Rgba([v, v, v, 1.0]);
        }
        let base = DynamicImage::ImageRgba32F(buf);

        let rendered = render_high_precision(
            &context,
            &base,
            RenderRequest {
                adjustments: neutral(),
                mask_bitmaps: &[],
                lut: None,
                roi: None,
            },
        )
        .expect("the tiled high-precision render should succeed");
        assert_eq!(rendered.dimensions(), (W, H));

        // Exactly what encode_image_to_bytes does for "tiff".
        let mut bytes = Vec::new();
        DynamicImage::ImageRgb16(rendered.to_rgb16())
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Tiff,
            )
            .expect("the TIFF should encode");

        let reopened = image::load_from_memory_with_format(&bytes, image::ImageFormat::Tiff)
            .expect("the TIFF we just wrote should reopen");
        assert_eq!(reopened.dimensions(), (W, H));
        let decoded = reopened
            .as_rgb16()
            .expect("the file came back as something other than 16-bit RGB");

        // Real 16-bit data, not 8-bit widened.
        let off_lattice = decoded.pixels().filter(|p| p[0] % 257 != 0).count();
        assert!(
            off_lattice > (W * H) as usize / 2,
            "only {off_lattice} of {} samples were off the 8-bit lattice after the \
             round trip",
            W * H,
        );

        // Monotonic along every row, including across the tile seam at x = 2048.
        for y in 0..H {
            let mut falls = 0;
            for x in 1..W {
                if decoded.get_pixel(x, y)[0] < decoded.get_pixel(x - 1, y)[0] {
                    falls += 1;
                }
            }
            assert!(
                falls < 16,
                "row {y} reverses {falls} times - the tiles are being reassembled \
                 at the wrong offsets",
            );
        }

        // And rows stay in order, which a wrong row stride would scramble.
        for y in 1..H {
            assert!(
                decoded.get_pixel(0, y)[0] >= decoded.get_pixel(0, y - 1)[0],
                "row {y} is darker than row {} at x=0, but the input brightens \
                 downwards",
                y - 1,
            );
        }
    }

    /// What actually limits the precision, asserted rather than eyeballed.
    ///
    /// THE MISTAKE THIS REPLACES
    ///
    /// The first version of this test printed "distinct levels" for one ramp
    /// width and called the result a ceiling. It is not a ceiling: half-float
    /// holds 1024 values in *every* octave, so a full-range ramp of W samples
    /// returns 1024 for each octave the ramp out-resolves plus everything below
    /// it, and the count climbs by 1024 on every doubling of W. Reading one
    /// number as "about 12 bits" produced a release headline that was wrong, and
    /// `log2(count)` of a sampling artefact is not a bit depth.
    ///
    /// WHAT IS TRUE
    ///
    /// The render is limited by the half-float upload and by nothing else. So the
    /// assertion is equality with a CPU half-float round-trip of the same ramp:
    /// if the GPU result matches it exactly, the pipeline adds no error of its
    /// own, and the precision of an export is exactly the precision of its input.
    ///
    /// This test is *expected to fail* the day the export input becomes f32 - see
    /// the roadmap. That is the point. It fails with a number that says how much
    /// better things got, rather than quietly passing because it only ever asked
    /// whether the output beat 8 bits.
    #[test]
    #[ignore = "needs a GPU; run with --ignored"]
    fn precision_is_limited_by_the_upload_and_by_nothing_else() {
        let Some(context) = device() else {
            panic!("no wgpu adapter - this test cannot tell you anything here");
        };

        for width in [2048u32, 4096, 8192, 16384] {
            let mut buf = ImageBuffer::<Rgba<f32>, Vec<f32>>::new(width, 1);
            for (x, _y, px) in buf.enumerate_pixels_mut() {
                let v = x as f32 / (width - 1) as f32;
                *px = Rgba([v, v, v, 1.0]);
            }

            let out = render_high_precision(
                &context,
                &DynamicImage::ImageRgba32F(buf),
                RenderRequest {
                    adjustments: neutral(),
                    mask_bitmaps: &[],
                    lut: None,
                    roi: None,
                },
            )
            .expect("the high-precision render should succeed");

            let encoded = out.to_rgb16();
            let rendered: std::collections::HashSet<u16> =
                (0..width).map(|x| encoded.get_pixel(x, 0)[0]).collect();

            // The same ramp, quantised to half-float on the CPU and nowhere else.
            let through_half: std::collections::HashSet<u16> = (0..width)
                .map(|x| {
                    let v = x as f32 / (width - 1) as f32;
                    sample_to_u16(half::f16::from_f32(v).to_f32())
                })
                .collect();

            assert_eq!(
                rendered.len(),
                through_half.len(),
                "at {width} samples the render produced {} distinct levels where a \
                 pure half-float round-trip gives {}. If the render is LOWER, \
                 something in the pipeline is losing precision the upload had not \
                 already lost. If it is HIGHER, the upload is no longer half-float \
                 and this test has done its job - update it and the roadmap.",
                rendered.len(),
                through_half.len(),
            );

            // And the floor, so this can never silently regress to 8-bit.
            assert!(
                rendered.len() > 1024,
                "only {} distinct levels at {width} samples - that is at or below \
                 8-bit territory",
                rendered.len(),
            );
        }
    }

    /// The same render at Preview precision must still be 8-bit, or the negative
    /// control above is measuring the ramp rather than the pipeline.
    #[test]
    #[ignore = "needs a GPU; run with --ignored"]
    fn the_preview_pipeline_is_still_eight_bit() {
        let Some(context) = device() else {
            panic!("no wgpu adapter - this test cannot tell you anything on this machine");
        };

        let base = fine_ramp(512, 8);
        let processor = GpuProcessor::new(context.clone(), 512, 256)
            .expect("the preview processor should build");

        let texels = crate::gpu_processing::to_rgba_f16(&base);
        let texture = context.device.create_texture_with_data(
            &context.queue,
            &wgpu::TextureDescriptor {
                label: Some("preview control input"),
                size: wgpu::Extent3d {
                    width: 512,
                    height: 8,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba16Float,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            TextureDataOrder::MipMajor,
            bytemuck::cast_slice(&texels),
        );
        let view = texture.create_view(&Default::default());

        let request = RenderRequest {
            adjustments: neutral(),
            mask_bitmaps: &[],
            lut: None,
            roi: None,
        };
        let (bytes, w, h, _, _) = processor
            .run(&view, 512, 8, request, false, false)
            .expect("the preview render should succeed");

        assert_eq!(
            bytes.len(),
            w as usize * h as usize * 4,
            "the preview path should still return four bytes a pixel",
        );

        // And the ramp genuinely collapses here, which is what makes the
        // high-precision result above mean something. Without this the test
        // proved only the byte count, while its name claimed 8 bits - the same
        // gap between a label and its contents that this whole feature is about.
        let row: Vec<u8> = (0..512).map(|x| bytes[x as usize * 4]).collect();
        let distinct = row.iter().collect::<std::collections::HashSet<_>>().len();
        assert!(
            distinct < 300,
            "512 samples of a ramp finer than 1/255 came back as {distinct} distinct              8-bit values; the ramp is too coarse to tell the two pipelines apart",
        );
    }
}
