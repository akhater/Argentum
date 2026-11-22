//! Preview JPEGs encoded in parallel strips.
//!
//! Horizontal strips are encoded on rayon threads and joined into one
//! baseline JPEG with a restart marker between strips. Strips start on
//! 16-row MCU boundaries and the fastest preset uses the standard Huffman
//! tables and no smoothing, so the joined data decodes to the same pixels as
//! one encode of the whole frame. Small frames, and strips without the
//! expected layout, fall back to one encode.
//!
//! Copied whole from RapidRAW; the tests are ours.

// RapidRAW 35fdc7c9: src-tauri/src/preview_jpeg.rs
use imgref::ImgRef;
use mozjpeg_rs::{Encoder, Preset};
use rayon::prelude::*;
use rgb::RGBA8;

const MCU_SIZE: usize = 16;
const MIN_STRIP_ROWS: usize = 128;

const SOI: u8 = 0xD8;
const EOI: u8 = 0xD9;
const SOF0: u8 = 0xC0;
const DHT: u8 = 0xC4;
const DRI: u8 = 0xDD;
const SOS: u8 = 0xDA;
const RST0: u8 = 0xD0;

/// `image` as a baseline JPEG, encoded in strips when it is tall enough.
pub fn encode_rgba(
    image: ImgRef<RGBA8>,
    quality: u8,
    fast_color: bool,
) -> mozjpeg_rs::Result<Vec<u8>> {
    let strips = (image.height() / MIN_STRIP_ROWS).min(rayon::current_num_threads());
    encode_in_strips(image, quality, fast_color, strips)
}

fn encode_single(
    image: ImgRef<RGBA8>,
    quality: u8,
    fast_color: bool,
) -> mozjpeg_rs::Result<Vec<u8>> {
    Encoder::new(Preset::BaselineFastest)
        .quality(quality)
        .fast_color(fast_color)
        .encode_imgref(image)
}

fn encode_in_strips(
    image: ImgRef<RGBA8>,
    quality: u8,
    fast_color: bool,
    strips: usize,
) -> mozjpeg_rs::Result<Vec<u8>> {
    let (width, height) = (image.width(), image.height());
    if strips < 2 || width == 0 || width > usize::from(u16::MAX) || height > usize::from(u16::MAX) {
        return encode_single(image, quality, fast_color);
    }
    let strip_rows = strip_rows(width, height, strips);
    let restart_interval = width.div_ceil(MCU_SIZE) * (strip_rows / MCU_SIZE);
    let Ok(restart_interval) = u16::try_from(restart_interval) else {
        return encode_single(image, quality, fast_color);
    };

    let tops: Vec<usize> = (0..height).step_by(strip_rows).collect();
    if tops.len() < 2 {
        return encode_single(image, quality, fast_color);
    }
    let parts = tops
        .into_par_iter()
        .map(|top| {
            let rows = strip_rows.min(height - top);
            encode_single(image.sub_image(0, top, width, rows), quality, fast_color)
        })
        .collect::<Result<Vec<_>, _>>()?;

    match join_strips(&parts, height, restart_interval) {
        Some(joined) => Ok(joined),
        None => encode_single(image, quality, fast_color),
    }
}

fn strip_rows(width: usize, height: usize, strips: usize) -> usize {
    let max_rows = usize::from(u16::MAX) / width.div_ceil(MCU_SIZE) * MCU_SIZE;
    height
        .div_ceil(strips)
        .next_multiple_of(MCU_SIZE)
        .min(max_rows)
}

struct Parsed<'a> {
    header: &'a [u8],
    sof_offset: usize,
    sos_segment: &'a [u8],
    entropy_data: &'a [u8],
}

fn parse(jpeg: &[u8]) -> Option<Parsed<'_>> {
    if jpeg.len() < 4 || jpeg[..2] != [0xFF, SOI] || jpeg[jpeg.len() - 2..] != [0xFF, EOI] {
        return None;
    }
    let mut pos = 2;
    let mut sof_offset = None;
    loop {
        if *jpeg.get(pos)? != 0xFF {
            return None;
        }
        let marker = *jpeg.get(pos + 1)?;
        let length = u16::from_be_bytes([*jpeg.get(pos + 2)?, *jpeg.get(pos + 3)?]) as usize;
        let segment = jpeg.get(pos..pos + 2 + length)?;
        match marker {
            SOF0 => sof_offset = Some(pos),
            DHT => {}
            DRI => return None,
            SOS => {
                return Some(Parsed {
                    header: &jpeg[..pos],
                    sof_offset: sof_offset?,
                    sos_segment: segment,
                    entropy_data: jpeg.get(pos + 2 + length..jpeg.len() - 2)?,
                });
            }
            0xC1..=0xCF => return None,
            _ => {}
        }
        pos += 2 + length;
    }
}

fn mcu_size(sof: &[u8]) -> Option<(usize, usize)> {
    let components = *sof.get(9)? as usize;
    let (h, v) = (0..components).try_fold((1, 1), |(h, v), i| {
        let factors = *sof.get(11 + 3 * i)?;
        Some((h.max(factors >> 4), v.max(factors & 0x0F)))
    })?;
    Some((8 * h as usize, 8 * v as usize))
}

fn join_strips(parts: &[Vec<u8>], height: usize, restart_interval: u16) -> Option<Vec<u8>> {
    let parsed = parts.iter().map(|p| parse(p)).collect::<Option<Vec<_>>>()?;
    let first = &parsed[0];
    let same_frame = |p: &Parsed| {
        p.sos_segment == first.sos_segment
            && p.header.len() == first.header.len()
            && p.sof_offset == first.sof_offset
            && p.header[..first.sof_offset + 5] == first.header[..first.sof_offset + 5]
            && p.header[first.sof_offset + 7..] == first.header[first.sof_offset + 7..]
    };
    if !parsed.iter().all(same_frame)
        || mcu_size(&first.header[first.sof_offset..])? != (MCU_SIZE, MCU_SIZE)
    {
        return None;
    }

    let entropy_len: usize = parsed.iter().map(|p| p.entropy_data.len() + 2).sum();
    let mut out =
        Vec::with_capacity(first.header.len() + 6 + first.sos_segment.len() + entropy_len);
    out.extend_from_slice(first.header);
    let height = u16::try_from(height).ok()?.to_be_bytes();
    out[first.sof_offset + 5..first.sof_offset + 7].copy_from_slice(&height);
    out.extend_from_slice(&[0xFF, DRI, 0, 4]);
    out.extend_from_slice(&restart_interval.to_be_bytes());
    out.extend_from_slice(first.sos_segment);
    for (i, part) in parsed.iter().enumerate() {
        if i > 0 {
            out.extend_from_slice(&[0xFF, RST0 + ((i - 1) % 8) as u8]);
        }
        out.extend_from_slice(part.entropy_data);
    }
    out.extend_from_slice(&[0xFF, EOI]);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rgb::FromSlice;

    /// A frame with smooth gradients, edges and noise, so every strip has
    /// real entropy data and DC values that differ between strips.
    fn frame(w: usize, h: usize) -> Vec<u8> {
        let mut state = 0x9e37_79b9u32;
        let mut px = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            for x in 0..w {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                let noise = (state % 23) as usize;
                let edge = if (x / 97 + y / 61) % 2 == 0 { 40 } else { 0 };
                px.push(((x * 255 / w + noise) % 256) as u8);
                px.push(((y * 255 / h + edge) % 256) as u8);
                px.push(((x + y) * 128 / (w + h) + noise) as u8);
                px.push(255);
            }
        }
        px
    }

    fn decoded(jpeg: &[u8]) -> Vec<u8> {
        image::load_from_memory_with_format(jpeg, image::ImageFormat::Jpeg)
            .expect("decodes")
            .into_rgb8()
            .into_raw()
    }

    fn has_restart_interval(jpeg: &[u8]) -> bool {
        jpeg.windows(2).any(|m| m == [0xFF, DRI])
    }

    /// The editor's tick (quality 80, fast colour) and the full-size preview
    /// (quality 92, from RGB before): strips decode to the same pixels as the
    /// one encode they replace, at sizes that end mid-MCU and mid-strip.
    #[test]
    fn strips_decode_to_the_same_pixels() {
        for (w, h) in [(1920, 1280), (1280, 1920), (1001, 777), (3000, 2003)] {
            let px = frame(w, h);
            let img = ImgRef::new(px.as_rgba(), w, h);
            for (quality, fast_color) in [(80, true), (92, false)] {
                let strips = encode_in_strips(img, quality, fast_color, 8).expect("encodes");
                assert!(
                    has_restart_interval(&strips),
                    "{w}x{h} q{quality}: fell back to one encode"
                );
                let one = encode_single(img, quality, fast_color).expect("encodes");
                assert!(
                    decoded(&strips) == decoded(&one),
                    "{w}x{h} q{quality}: strips decode differently"
                );
            }
            let rgb: Vec<u8> = px
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|p| [p[0], p[1], p[2]])
                .collect();
            let from_rgb = Encoder::new(Preset::BaselineFastest)
                .quality(92)
                .encode_rgb(&rgb, w as u32, h as u32)
                .expect("encodes");
            let strips = encode_in_strips(img, 92, false, 8).expect("encodes");
            assert!(
                decoded(&strips) == decoded(&from_rgb),
                "{w}x{h}: RGBA strips decode differently from the RGB encode"
            );
        }
    }

    #[test]
    fn a_small_frame_is_one_encode() {
        let px = frame(64, 64);
        let img = ImgRef::new(px.as_rgba(), 64, 64);
        let jpeg = encode_rgba(img, 80, true).expect("encodes");
        assert!(!has_restart_interval(&jpeg));
    }

    /// One encode against strips, at the editor's preview size and full size.
    /// `cargo test --profile measure --lib mods::preview_jpeg:: -- --ignored --nocapture`
    #[test]
    #[ignore = "timing, run by hand"]
    fn encode_time() {
        for (w, h, quality, fast_color) in [(1920, 1280, 80, true), (6000, 4000, 92, false)] {
            let px = frame(w, h);
            let img = ImgRef::new(px.as_rgba(), w, h);
            let median = |f: &dyn Fn() -> Vec<u8>| {
                let mut ms: Vec<f64> = (0..9)
                    .map(|_| {
                        let at = std::time::Instant::now();
                        std::hint::black_box(f());
                        at.elapsed().as_secs_f64() * 1e3
                    })
                    .collect();
                ms.sort_by(f64::total_cmp);
                ms[ms.len() / 2]
            };
            let one = median(&|| encode_single(img, quality, fast_color).unwrap());
            let strips = median(&|| encode_rgba(img, quality, fast_color).unwrap());
            println!(
                "{w}x{h} q{quality}: one encode {one:.1} ms, strips {strips:.1} ms ({} threads)",
                rayon::current_num_threads()
            );
        }
    }
}
