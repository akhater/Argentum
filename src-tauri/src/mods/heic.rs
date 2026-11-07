//! HEIC and HEIF photos (an iPhone's), decoded by the system's own HEVC
//! decoder, so Argentum ships none: Windows Imaging Component (`wic`), macOS's
//! ImageIO (`imageio`), and elsewhere, as on Linux, the system's libheif
//! (`libheif`), opened when first needed.
//!
//! The header is read here, the same on every system: the primary image's
//! size, crop (`clap`), turn and mirror (`irot`, `imir`) and colour (`colr`).
//! The system decoder only decodes the pixels, unturned, in the file's own
//! colour space; they are cropped and turned here, and converted to sRGB by
//! moxcms, so a Display P3 photo keeps its colour. The result is what a JPEG
//! gives (`image_loader::load_image_with_orientation`): sRGB-encoded floats.
//!
//! When the system cannot decode it, the photo is still listed, and opening
//! it says what to install ([`HeicError`]). Depth maps, gain maps, alpha,
//! Live Photo video and the other images of a burst are not shown: only the
//! primary image.
//!
//! Adapted from RAWmakase (MIT), v0.4.12 (`704ab207`):
//! `crates/rawmakase-decode/src/raster/heic.rs` (layout, crop, limits),
//! `raster/heif.rs` (colour and orientation), `raster/heic/wic.rs`,
//! `raster/heic/imageio.rs`, `raster/heic/libheif.rs`, and the box reading of
//! `crates/rawmakase-interop/src/isobmff.rs`, cut to the boxes read here and
//! reading from memory. Not taken: its HEVC parameter-set reader (bounding the
//! coded size before decoding) and alpha images.
// Every unsafe operation in the decoders crosses into a C API: each says why
// it is sound.
#![deny(unsafe_op_in_unsafe_fn, clippy::undocumented_unsafe_blocks)]

use image::{DynamicImage, Rgb32FImage};
use moxcms::{
    CicpColorPrimaries, CicpProfile, ColorProfile, Layout, MatrixCoefficients,
    TransferCharacteristics, TransformOptions,
};
use rawler::Orientation;
use std::fmt;

#[cfg(target_os = "macos")]
mod imageio;
#[cfg(target_os = "macos")]
use imageio::pixels;
#[cfg(windows)]
mod wic;
#[cfg(windows)]
use wic::pixels;
#[cfg(all(unix, not(target_vendor = "apple")))]
mod libheif;
#[cfg(all(unix, not(target_vendor = "apple")))]
use libheif::pixels;
/// No decoder on this system.
#[cfg(not(any(unix, windows)))]
fn pixels(_: &Header, _: &[u8]) -> Result<Pixels, HeicError> {
    Err(HeicError::NoDecoder(
        "Argentum can't show HEIC photos on this system".into(),
    ))
}

/// Why a HEIC photo did not open. The message is what the photo's error
/// says, so it names what to do.
#[derive(Debug, Clone, PartialEq)]
pub enum HeicError {
    /// The system has no decoder for it, or its decoder failed: what to
    /// install.
    NoDecoder(String),
    /// A HEIF Argentum does not show: HDR, or not HEVC.
    Unsupported(&'static str),
    /// The file is not a HEIF it can read: cut off, or corrupt.
    Corrupt(&'static str),
}

impl fmt::Display for HeicError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoDecoder(why) => f.write_str(why),
            Self::Unsupported(what) => write!(f, "Argentum can't open {what} HEIF photos yet"),
            Self::Corrupt(why) => write!(f, "This HEIC photo can't be read: {why}"),
        }
    }
}

impl std::error::Error for HeicError {}

/// Whether `bytes` are a HEIF of HEVC (a HEIC), by its `ftyp` box: an HEVC
/// brand, or HEIF's own without AVIF's (an AVIF is AV1).
pub fn is_heif(bytes: &[u8]) -> bool {
    const HEVC: [&[u8; 4]; 8] = [
        b"heic", b"heix", b"heim", b"heis", b"hevc", b"hevx", b"hevm", b"hevs",
    ];
    const HEIF: [&[u8; 4]; 3] = [b"mif1", b"msf1", b"miaf"];
    const AVIF: [&[u8; 4]; 2] = [b"avif", b"avis"];
    let brands = brands(bytes);
    brands.iter().any(|b| HEVC.contains(b))
        || (brands.iter().any(|b| HEIF.contains(b)) && !brands.iter().any(|b| AVIF.contains(b)))
}

/// The major and compatible brands of the `ftyp` box `bytes` start with, as
/// far as its first 64 bytes go (not its minor version).
fn brands(bytes: &[u8]) -> Vec<&[u8; 4]> {
    let body = match bytes.get(4..8) {
        Some(b"ftyp") => bytes.get(8..bytes.len().min(64)).unwrap_or_default(),
        _ => &[],
    };
    body.as_chunks::<4>()
        .0
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != 1)
        .map(|(_, b)| b)
        .collect()
}

/// The primary image of a HEIC, upright and in sRGB, as sRGB-encoded floats.
pub fn decode(bytes: &[u8]) -> Result<DynamicImage, HeicError> {
    let header = Header::read(bytes)?;
    let decoded = pixels(&header, bytes)?;
    let rgb = header.to_srgb(cropped(&header, decoded)?)?;
    let image = Rgb32FImage::from_raw(header.crop.width, header.crop.height, rgb)
        .ok_or(HeicError::Corrupt("the picture is not the size it says"))?;
    Ok(crate::image_processing::apply_orientation(
        DynamicImage::ImageRgb32F(image),
        header.orientation,
    ))
}

/// The most pixels a HEIC image may have, as RAWmakase holds it to.
const MAX_PIXELS: u64 = 150_000_000;

/// Decoded pixels: `width` × `height` of interleaved RGB, 8-bit samples or
/// 16-bit ones (native endian), as [`Header::deep`] says, in the colour space
/// [`Header::icc`] describes: the whole picture, or (a system decoder that
/// crops by the `clap` itself) the clean aperture alone.
struct Pixels {
    width: u32,
    height: u32,
    rgb: Vec<u8>,
}

/// Where the image is in the decoded picture: its clean aperture.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Crop {
    left: u32,
    top: u32,
    width: u32,
    height: u32,
}

/// What a HEIC's header says of its primary image.
#[derive(Debug)]
struct Header {
    /// The whole picture's size, before its crop.
    width: u32,
    height: u32,
    crop: Crop,
    /// More than 8 bits per sample, from `hvcC`.
    deep: bool,
    /// The ICC profile of the pixels: the file's, or one built from its CICP
    /// code points; `None` for sRGB.
    icc: Option<Vec<u8>>,
    orientation: Orientation,
}

impl Header {
    fn read(bytes: &[u8]) -> Result<Self, HeicError> {
        let heif = Heif::read(bytes).ok_or(HeicError::Corrupt("its header is damaged"))?;
        let kind = heif
            .item(heif.primary)
            .ok_or(HeicError::Corrupt("it has no image"))?;
        if !matches!(kind, b"hvc1" | b"grid") {
            return Err(HeicError::Unsupported("non-HEVC"));
        }
        let props = heif.properties(heif.primary);
        let (width, height) = props
            .iter()
            .find_map(|p| match p {
                Property::Size(w, h) => Some((*w, *h)),
                _ => None,
            })
            .filter(|(w, h)| *w > 0 && *h > 0)
            .ok_or(HeicError::Corrupt("its image has no size"))?;
        if u64::from(width) * u64::from(height) > MAX_PIXELS {
            return Err(HeicError::Unsupported("over 150-megapixel"));
        }
        // A grid's tiles carry the HEVC configuration, and an iPhone's carry
        // the `nclx` too.
        let tile = match kind {
            b"grid" => *heif
                .referenced(*b"dimg", heif.primary)
                .first()
                .ok_or(HeicError::Corrupt("its grid has no tiles"))?,
            _ => heif.primary,
        };
        let tile_props = heif.properties(tile);
        let deep = tile_props
            .iter()
            .find_map(|p| match p {
                Property::HevcConfig(c) => hevc_depth(c),
                _ => None,
            })
            .ok_or(HeicError::Corrupt("it has no HEVC configuration"))?
            > 8;
        let (icc, nclx) = colour(&props);
        let (tile_icc, tile_nclx) = colour(&tile_props);
        let (icc, nclx) = (icc.or(tile_icc), nclx.or(tile_nclx));
        if nclx.is_some_and(Nclx::hdr) {
            return Err(HeicError::Unsupported("HDR"));
        }
        Ok(Self {
            width,
            height,
            crop: crop(&props, width, height)?,
            deep,
            icc: profile(icc, nclx),
            orientation: transforms(&props),
        })
    }

    /// Whether a picture of `width` × `height` is this image: the whole
    /// picture, or its crop alone. A system decoder's size is checked so
    /// before anything that size is allocated.
    #[cfg_attr(not(any(unix, windows)), allow(dead_code))]
    fn expects(&self, width: u32, height: u32) -> bool {
        (width, height) == (self.width, self.height)
            || (width, height) == (self.crop.width, self.crop.height)
    }

    /// The crop's samples, from the file's colour space to sRGB, 0 to 1.
    fn to_srgb(&self, samples: Vec<f32>) -> Result<Vec<f32>, HeicError> {
        let Some(icc) = &self.icc else {
            return Ok(samples);
        };
        let profile = ColorProfile::new_from_slice(icc)
            .map_err(|_| HeicError::Corrupt("its colour profile is damaged"))?;
        let transform = profile
            .create_transform_f32(
                Layout::Rgb,
                &ColorProfile::new_srgb(),
                Layout::Rgb,
                TransformOptions::default(),
            )
            .map_err(|_| HeicError::Corrupt("its colour profile is not an RGB one"))?;
        let mut out = vec![0.0; samples.len()];
        transform
            .transform(&samples, &mut out)
            .map_err(|_| HeicError::Corrupt("its colours can't be converted"))?;
        out.iter_mut().for_each(|v| *v = v.clamp(0.0, 1.0));
        Ok(out)
    }
}

/// The image's crop of `decoded`, as samples from 0 to 1: the whole picture
/// cropped here, or the crop alone as decoded. Any other size is not the
/// image the header describes (a system decoder that turned it, say).
fn cropped(header: &Header, decoded: Pixels) -> Result<Vec<f32>, HeicError> {
    let Pixels { width, height, rgb } = decoded;
    let crop = header.crop;
    let (left, top) = if (width, height) == (header.width, header.height) {
        (crop.left, crop.top)
    } else if (width, height) == (crop.width, crop.height) {
        (0, 0)
    } else {
        return Err(HeicError::Corrupt("the picture is not the size it says"));
    };
    let sample = if header.deep { 2 } else { 1 };
    let (row, kept) = (width as usize * 3, crop.width as usize * 3);
    if rgb.len() != row * height as usize * sample {
        return Err(HeicError::Corrupt("the picture is not the size it says"));
    }
    let mut out = Vec::with_capacity(kept * crop.height as usize);
    for y in 0..crop.height as usize {
        let start = (top as usize + y) * row + left as usize * 3;
        let bytes = &rgb[start * sample..(start + kept) * sample];
        if header.deep {
            out.extend(
                bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|s| f32::from(u16::from_ne_bytes(*s)) / 65535.0),
            );
        } else {
            out.extend(bytes.iter().map(|v| f32::from(*v) / 255.0));
        }
    }
    Ok(out)
}

/// The bit depth of luma an `hvcC` record codes (ISO/IEC 14496-15 8.3.3.1:
/// `bitDepthLumaMinus8` in the low 3 bits of its 18th byte).
fn hevc_depth(hvcc: &[u8]) -> Option<u8> {
    Some((hvcc.get(17)? & 7) + 8)
}

/// The CICP code points of an `nclx` box.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Nclx {
    primaries: u16,
    transfer: u16,
}

impl Nclx {
    /// An HDR transfer: PQ or HLG.
    fn hdr(self) -> bool {
        matches!(self.transfer, 16 | 18)
    }
}

/// An item's colour: its ICC profile and its `nclx`, either when it has one.
fn colour(props: &[&Property]) -> (Option<Vec<u8>>, Option<Nclx>) {
    let icc = props.iter().find_map(|p| match p {
        Property::Icc(icc) => Some(icc.clone()),
        _ => None,
    });
    let nclx = props.iter().find_map(|p| match p {
        Property::Nclx(n) => Some(*n),
        _ => None,
    });
    (icc, nclx)
}

/// The ICC profile describing an item's pixels: its own, else one built
/// from its CICP code points; `None` for sRGB.
fn profile(icc: Option<Vec<u8>>, nclx: Option<Nclx>) -> Option<Vec<u8>> {
    match (icc, nclx) {
        (Some(icc), _) => Some(icc),
        (None, Some(n)) => cicp_profile(n.primaries, n.transfer).and_then(|p| p.encode().ok()),
        (None, None) => None,
    }
}

/// A profile for CICP `primaries` and `transfer`, unspecified ones taken as
/// sRGB's (as libavif does); `None` for sRGB itself or primaries moxcms can't
/// describe. A transfer it can't describe is taken as sRGB's.
fn cicp_profile(primaries: u16, transfer: u16) -> Option<ColorProfile> {
    let primaries = if primaries == 2 { 1 } else { primaries };
    let transfer = if transfer == 2 { 13 } else { transfer };
    if (primaries, transfer) == (1, 13) {
        return None;
    }
    let build = |transfer| {
        let cicp = CicpProfile {
            color_primaries: CicpColorPrimaries::try_from(u8::try_from(primaries).ok()?).ok()?,
            transfer_characteristics: transfer,
            matrix_coefficients: MatrixCoefficients::Bt709,
            full_range: true,
        };
        let mut profile = ColorProfile::new_srgb();
        // moxcms 0.8 returns false even when it updated the profile; whether
        // it did is in `cicp`.
        profile.update_rgb_colorimetry_from_cicp(cicp);
        profile.cicp.is_some().then_some(profile)
    };
    u8::try_from(transfer)
        .ok()
        .and_then(|t| TransferCharacteristics::try_from(t).ok())
        .and_then(build)
        .or_else(|| build(TransferCharacteristics::Srgb))
}

/// The orientation an item's `irot` and `imir` give, applied in the order
/// they are associated; a turn after a mirror is the opposite turn before it.
fn transforms(props: &[&Property]) -> Orientation {
    let (mut rotation, mut mirror) = (0, None);
    for p in props {
        match p {
            Property::Rotation(r) if mirror.is_some() => rotation = r.wrapping_neg() & 3,
            Property::Rotation(r) => rotation = *r,
            Property::Mirror(m) => mirror = Some(*m),
            _ => {}
        }
    }
    orientation(rotation, mirror)
}

/// `irot` (anticlockwise quarter turns) then `imir` (0: top and bottom swap,
/// 1: left and right, ISO/IEC 23008-12:2022) as the EXIF orientation that
/// puts the picture upright.
fn orientation(rotation: u8, mirror: Option<u8>) -> Orientation {
    use Orientation::*;
    match (rotation & 3, mirror.map(|m| m & 1)) {
        (0, None) => Normal,
        (1, None) => Rotate270,
        (2, None) => Rotate180,
        (3, None) => Rotate90,
        (0, Some(1)) | (2, Some(0)) => HorizontalFlip,
        (0, Some(_)) | (2, Some(_)) => VerticalFlip,
        (1, Some(1)) | (3, Some(0)) => Transverse,
        (_, Some(_)) => Transpose,
        _ => unreachable!("a rotation is masked to 0..=3"),
    }
}

/// The part of a `width` × `height` picture a `clap` keeps: its size rounded,
/// its left and top edges rounded down, as libheif crops; the whole picture
/// without one.
fn crop(props: &[&Property], width: u32, height: u32) -> Result<Crop, HeicError> {
    let Some(clap) = props.iter().find_map(|p| match p {
        Property::CleanAperture(c) => Some(*c),
        _ => None,
    }) else {
        return Ok(Crop {
            left: 0,
            top: 0,
            width,
            height,
        });
    };
    let round = |(n, d): (u32, u32)| (i128::from(n) + i128::from(d) / 2) / i128::from(d);
    // The centre's offset, plus half of what the crop leaves out:
    // offset + (full - size) / 2, over the denominator 2 × od × sd.
    let edge = |(on, od): (i32, u32), full: u32, (sn, sd): (u32, u32)| {
        let (on, od, full) = (i128::from(on), i128::from(od), i128::from(full));
        let (sn, sd) = (i128::from(sn), i128::from(sd));
        (2 * on * sd + full * od * sd - sn * od).div_euclid(2 * od * sd)
    };
    let (w, h) = (round(clap.width), round(clap.height));
    let left = edge(clap.horizontal_offset, width, clap.width);
    let top = edge(clap.vertical_offset, height, clap.height);
    if !(w > 0
        && h > 0
        && left >= 0
        && top >= 0
        && left + w <= width.into()
        && top + h <= height.into())
    {
        return Err(HeicError::Corrupt("its crop is outside the picture"));
    }
    // Each is within `width` or `height`, so fits.
    Ok(Crop {
        left: left as u32,
        top: top as u32,
        width: w as u32,
        height: h as u32,
    })
}

/// An item property read here; others are skipped.
#[derive(Clone, Debug, PartialEq)]
enum Property {
    /// `hvcC`: the HEVC configuration record.
    HevcConfig(Vec<u8>),
    /// `colr` of an ICC profile (`prof`, `rICC`).
    Icc(Vec<u8>),
    /// `colr` of CICP code points (`nclx`).
    Nclx(Nclx),
    /// `ispe`: width and height.
    Size(u32, u32),
    /// `irot`: anticlockwise quarter turns, 0–3.
    Rotation(u8),
    /// `imir`: 0 swaps the top and bottom, 1 the left and right.
    Mirror(u8),
    /// `clap`: a crop.
    CleanAperture(CleanAperture),
    Other,
}

/// A `clap` box: the size of the image kept, and how far its centre is from
/// the full image's, each a fraction (numerator, denominator).
#[derive(Clone, Copy, Debug, PartialEq)]
struct CleanAperture {
    width: (u32, u32),
    height: (u32, u32),
    horizontal_offset: (i32, u32),
    vertical_offset: (i32, u32),
}

/// The most boxes, associations or references read from one list.
const MAX_BOXES: usize = 4096;

/// What a HEIF's `meta` box says: its primary item, each item's kind, the
/// properties and which items they belong to, and the references between
/// items.
#[derive(Debug, Default)]
struct Heif {
    primary: u32,
    items: Vec<(u32, [u8; 4])>,
    properties: Vec<Property>,
    /// Each item's properties, as 1-based indexes into `properties`.
    associations: Vec<(u32, Vec<u16>)>,
    /// (kind, from, to).
    references: Vec<([u8; 4], u32, u32)>,
}

impl Heif {
    /// The `meta` of a HEIF file; `None` when it has none it can read, or
    /// the file is cut off (a box runs past its end): the header, not the
    /// system decoder, says so, as a decoder may make up the missing pixels.
    fn read(bytes: &[u8]) -> Option<Self> {
        let (boxes, whole) = top_level(bytes);
        if !whole {
            return None;
        }
        let meta = boxes.into_iter().find(|(kind, _)| kind == b"meta")?.1;
        let mut m = Bytes(meta);
        m.take(4)?; // version and flags
        let mut heif = Heif::default();
        for (kind, body) in m.children() {
            let mut b = Bytes(body);
            match &kind {
                b"pitm" => {
                    let version = b.u8()?;
                    b.take(3)?;
                    heif.primary = if version == 0 {
                        b.u16()?.into()
                    } else {
                        b.u32()?
                    };
                }
                b"iinf" => heif.items = items(b)?,
                b"iprp" => heif.read_properties(b)?,
                b"iref" => heif.references = references(b)?,
                _ => {}
            }
        }
        Some(heif)
    }

    fn item(&self, id: u32) -> Option<&[u8; 4]> {
        self.items.iter().find(|(i, _)| *i == id).map(|(_, k)| k)
    }

    /// The properties of item `id`, in the order its association lists them
    /// (the order transformations apply in).
    fn properties(&self, id: u32) -> Vec<&Property> {
        self.associations
            .iter()
            .filter(|(item, _)| *item == id)
            .flat_map(|(_, indexes)| indexes.iter())
            .filter_map(|i| self.properties.get((*i as usize).checked_sub(1)?))
            .collect()
    }

    /// The items item `from` has a `kind` reference to, in the order it lists
    /// them: for "dimg" from a grid, its tiles row by row.
    fn referenced(&self, kind: [u8; 4], from: u32) -> Vec<u32> {
        self.references
            .iter()
            .filter(|(k, f, _)| *k == kind && *f == from)
            .map(|(_, _, to)| *to)
            .collect()
    }

    fn read_properties(&mut self, b: Bytes) -> Option<()> {
        for (kind, body) in b.children() {
            match &kind {
                b"ipco" => {
                    self.properties = Bytes(body)
                        .children()
                        .into_iter()
                        .map(|(kind, body)| property(kind, body).unwrap_or(Property::Other))
                        .collect()
                }
                // One per version and flags, so there may be several.
                b"ipma" if self.associations.len() < MAX_BOXES => {
                    self.associations.extend(associations(Bytes(body))?);
                    self.associations.truncate(MAX_BOXES);
                }
                _ => {}
            }
        }
        Some(())
    }
}

/// A box: its kind and its body.
type Boxed<'a> = ([u8; 4], &'a [u8]);

/// The file's top-level boxes, each kind and body, and whether they fill it
/// to its end. A box's size may be 64-bit (1) or run to the end of the file
/// (0).
fn top_level(bytes: &[u8]) -> (Vec<Boxed<'_>>, bool) {
    let mut found = Vec::new();
    let mut at = 0usize;
    while found.len() < MAX_BOXES {
        if at == bytes.len() {
            return (found, true);
        }
        let Some(head) = bytes.get(at..at + 8) else {
            break;
        };
        let kind: [u8; 4] = head[4..8].try_into().unwrap_or_default();
        let (header, total) = match u32::from_be_bytes(head[..4].try_into().unwrap_or_default()) {
            0 => (8, bytes.len() - at),
            1 => match bytes.get(at + 8..at + 16) {
                Some(large) => (
                    16,
                    usize::try_from(u64::from_be_bytes(large.try_into().unwrap_or_default()))
                        .unwrap_or(usize::MAX),
                ),
                None => break,
            },
            n => (8, n as usize),
        };
        let Some(body) = at
            .checked_add(total)
            .filter(|end| total >= header && *end <= bytes.len())
            .map(|end| &bytes[at + header..end])
        else {
            break;
        };
        found.push((kind, body));
        at += total;
    }
    (found, false)
}

/// A big-endian cursor over a box body; every read is bounds-checked.
struct Bytes<'a>(&'a [u8]);

impl<'a> Bytes<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        (self.0.len() >= n).then(|| {
            let (head, rest) = self.0.split_at(n);
            self.0 = rest;
            head
        })
    }
    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }
    fn u16(&mut self) -> Option<u16> {
        self.take(2).map(|b| u16::from_be_bytes([b[0], b[1]]))
    }
    fn u32(&mut self) -> Option<u32> {
        self.take(4)
            .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
    /// A fraction: a numerator then a denominator other than 0.
    fn fraction(&mut self) -> Option<(u32, u32)> {
        let (n, d) = (self.u32()?, self.u32()?);
        (d != 0).then_some((n, d))
    }
    fn kind(&mut self) -> Option<[u8; 4]> {
        self.take(4).map(|b| [b[0], b[1], b[2], b[3]])
    }
    /// The boxes in the rest of the body.
    fn children(&self) -> Vec<Boxed<'a>> {
        let mut out = Vec::new();
        let mut rest = self.0;
        while rest.len() >= 8 && out.len() < MAX_BOXES {
            let size = u32::from_be_bytes([rest[0], rest[1], rest[2], rest[3]]) as usize;
            if size < 8 || size > rest.len() {
                break;
            }
            out.push(([rest[4], rest[5], rest[6], rest[7]], &rest[8..size]));
            rest = &rest[size..];
        }
        out
    }
}

/// `iinf`: each `infe` (version 2 or 3) item's id and kind.
fn items(mut b: Bytes) -> Option<Vec<(u32, [u8; 4])>> {
    let version = b.u8()?;
    b.take(3)?;
    let count = if version == 0 {
        b.u16()?.into()
    } else {
        b.u32()?
    };
    let mut out = Vec::new();
    for (kind, body) in b.children().into_iter().take(count as usize) {
        if &kind != b"infe" {
            continue;
        }
        let mut e = Bytes(body);
        let version = e.u8()?;
        e.take(3)?;
        if version < 2 {
            continue;
        }
        let id = if version == 2 {
            e.u16()?.into()
        } else {
            e.u32()?
        };
        e.u16()?; // protection index
        out.push((id, e.kind()?));
    }
    Some(out)
}

/// `ipma`: each item's properties, as 1-based indexes into `ipco`.
fn associations(mut b: Bytes) -> Option<Vec<(u32, Vec<u16>)>> {
    let version = b.u8()?;
    let flags = b.take(3)?[2];
    let count = b.u32()?;
    let mut out = Vec::new();
    for _ in 0..count.min(MAX_BOXES as u32) {
        let id = if version < 1 {
            b.u16()?.into()
        } else {
            b.u32()?
        };
        let n = b.u8()?;
        let mut indexes = Vec::new();
        for _ in 0..n {
            // The top bit says whether the property is essential.
            indexes.push(if flags & 1 == 1 {
                b.u16()? & 0x7fff
            } else {
                (b.u8()? & 0x7f).into()
            });
        }
        out.push((id, indexes));
    }
    Some(out)
}

/// `iref`: each reference's kind, from and to.
fn references(mut b: Bytes) -> Option<Vec<([u8; 4], u32, u32)>> {
    let version = b.u8()?;
    b.take(3)?;
    let mut out = Vec::new();
    for (kind, body) in b.children() {
        let mut r = Bytes(body);
        let id = |r: &mut Bytes| {
            if version == 0 {
                r.u16().map(u32::from)
            } else {
                r.u32()
            }
        };
        let from = id(&mut r)?;
        let n = r.u16()?;
        for _ in 0..n {
            out.push((kind, from, id(&mut r)?));
            if out.len() >= MAX_BOXES {
                return Some(out);
            }
        }
    }
    Some(out)
}

fn property(kind: [u8; 4], body: &[u8]) -> Option<Property> {
    let mut b = Bytes(body);
    Some(match &kind {
        b"hvcC" => Property::HevcConfig(body.to_vec()),
        b"colr" => match &b.kind()? {
            // The matrix and range are the system decoder's to apply.
            b"nclx" => Property::Nclx(Nclx {
                primaries: b.u16()?,
                transfer: b.u16()?,
            }),
            b"prof" | b"rICC" => Property::Icc(b.0.to_vec()),
            _ => Property::Other,
        },
        b"ispe" => {
            b.take(4)?;
            Property::Size(b.u32()?, b.u32()?)
        }
        b"irot" => Property::Rotation(b.u8()? & 3),
        b"imir" => Property::Mirror(b.u8()? & 1),
        b"clap" => {
            let (width, height) = (b.fraction()?, b.fraction()?);
            let (horizontal, vertical) = (b.fraction()?, b.fraction()?);
            Property::CleanAperture(CleanAperture {
                width,
                height,
                // The offsets are signed.
                horizontal_offset: (horizontal.0 as i32, horizontal.1),
                vertical_offset: (vertical.0 as i32, vertical.1),
            })
        }
        _ => Property::Other,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Vec<u8> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/heic")
            .join(name);
        std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    /// Whether this system decodes HEIC, so the tests that need its decoder
    /// can run. Where it can't they say so and pass, unless `AG_REQUIRE_HEIC`
    /// is set: then the decoder must be there.
    fn decoder_here() -> bool {
        match decode(&fixture("metadata.heic")) {
            Err(HeicError::NoDecoder(why)) => {
                assert!(
                    std::env::var_os("AG_REQUIRE_HEIC").is_none(),
                    "no HEIC decoder: {why}"
                );
                eprintln!("HEIC decoding not tested here: {why}");
                false
            }
            _ => true,
        }
    }

    /// The colour of the pixel at `x`, `y`, 8-bit.
    fn at(image: &DynamicImage, x: u32, y: u32) -> [u8; 3] {
        let p = image.as_rgb32f().expect("RGB floats").get_pixel(x, y);
        p.0.map(|v| (v * 255.0).round() as u8)
    }

    /// Whether `got` is `want` within a lossy codec's error.
    fn near(got: [u8; 3], want: [u8; 3]) -> bool {
        got.iter()
            .zip(want)
            .all(|(g, w)| (i16::from(*g) - i16::from(w)).abs() <= 24)
    }

    const RED: [u8; 3] = [255, 0, 0];
    const GREEN: [u8; 3] = [0, 255, 0];
    const BLUE: [u8; 3] = [0, 0, 255];
    const WHITE: [u8; 3] = [255, 255, 255];

    /// The four corners of `image`, clockwise from the top left, each taken
    /// one pixel in from the edge.
    fn corners(image: &DynamicImage) -> [[u8; 3]; 4] {
        let (w, h) = (image.width(), image.height());
        [
            at(image, 1, 1),
            at(image, w - 2, 1),
            at(image, w - 2, h - 2),
            at(image, 1, h - 2),
        ]
    }

    fn assert_corners(image: &DynamicImage, want: [[u8; 3]; 4]) {
        let got = corners(image);
        assert!(
            got.iter().zip(want).all(|(g, w)| near(*g, w)),
            "corners {got:?}, not {want:?}"
        );
    }

    /// Listed, whatever the system can decode, as a JPEG is.
    #[test]
    fn heic_photos_are_listed() {
        for name in ["IMG_0001.HEIC", "a.heic", "b.heif", "DSCF0001.HIF"] {
            assert!(crate::formats::is_supported_image_file(name), "{name}");
            assert!(!crate::formats::is_raw_file(name), "{name}");
        }
    }

    #[test]
    fn heic_files_are_told_by_their_brands() {
        for name in ["metadata.heic", "grid.heic", "p3.heic", "rotated.heic"] {
            assert!(is_heif(&fixture(name)), "{name}");
        }
        let avif = [
            &[0, 0, 0, 0x1c][..],
            b"ftyp",
            b"avif",
            &[0; 4],
            b"avif",
            b"mif1",
            b"miaf",
        ]
        .concat();
        assert!(!is_heif(&avif));
        assert!(!is_heif(b"\xff\xd8\xff\xe0 a JPEG"));
        assert!(!is_heif(b""));
    }

    #[test]
    fn the_header_says_the_size_crop_and_depth() {
        let header = Header::read(&fixture("metadata.heic")).unwrap();
        assert_eq!((header.width, header.height), (64, 64));
        assert_eq!(
            header.crop,
            Crop {
                left: 0,
                top: 0,
                width: 16,
                height: 12
            }
        );
        assert!(!header.deep);
        assert!(Header::read(&fixture("ten-bit.heic")).unwrap().deep);
    }

    #[test]
    fn a_grids_size_is_its_own_and_its_colour_its_tiles_when_it_has_none() {
        let header = Header::read(&fixture("grid.heic")).unwrap();
        assert_eq!((header.crop.width, header.crop.height), (120, 100));
        // The grid's own ICC profile, over the tiles' nclx (sRGB's, which
        // would give none).
        assert!(header.icc.is_some());
    }

    #[test]
    fn turns_and_mirrors_are_read_in_order() {
        let turned = Header::read(&fixture("rotated.heic")).unwrap();
        assert_eq!(turned.orientation, Orientation::Rotate270);
        let mirrored = Header::read(&fixture("rotated-mirrored.heic")).unwrap();
        assert_eq!(mirrored.orientation, Orientation::Transpose);
        let plain = Header::read(&fixture("metadata.heic")).unwrap();
        assert_eq!(plain.orientation, Orientation::Normal);
    }

    #[test]
    fn rotations_and_mirrors_turn_into_orientations() {
        use Orientation::*;
        let o = orientation;
        assert_eq!(o(0, None), Normal);
        assert_eq!(o(1, None), Rotate270);
        assert_eq!(o(2, None), Rotate180);
        assert_eq!(o(3, None), Rotate90);
        assert_eq!(o(0, Some(0)), VerticalFlip);
        assert_eq!(o(0, Some(1)), HorizontalFlip);
        assert_eq!(o(1, Some(0)), Transpose);
        assert_eq!(o(1, Some(1)), Transverse);
        assert_eq!(o(2, Some(0)), HorizontalFlip);
        assert_eq!(o(2, Some(1)), VerticalFlip);
        assert_eq!(o(3, Some(0)), Transverse);
        assert_eq!(o(3, Some(1)), Transpose);
    }

    #[test]
    fn display_p3_is_described_and_srgb_is_not() {
        let p3 = Header::read(&fixture("p3.heic")).unwrap();
        let profile = ColorProfile::new_from_slice(p3.icc.as_ref().expect("a P3 profile")).unwrap();
        let want = ColorProfile::new_display_p3();
        assert!((profile.red_colorant.x - want.red_colorant.x).abs() < 1e-3);
        assert!(cicp_profile(2, 2).is_none());
        assert!(cicp_profile(1, 13).is_none());
    }

    /// P3's red is outside sRGB: converted, it is sRGB's red, clipped; taken
    /// as sRGB it would stay P3's numbers. Its grey is grey either way.
    #[test]
    fn display_p3_samples_are_converted_to_srgb() {
        let header = Header::read(&fixture("p3.heic")).unwrap();
        let out = header
            .to_srgb(vec![1.0, 0.0, 0.0, 0.5, 0.5, 0.5, 0.5, 0.25, 0.25])
            .unwrap();
        assert!(out[0] > 0.99 && out[1] < 0.01 && out[2] < 0.01, "{out:?}");
        assert!(out[3..6].iter().all(|v| (v - 0.5).abs() < 0.01), "{out:?}");
        // A P3 colour inside sRGB is more saturated there.
        assert!(out[6] > 0.5 && out[7] < 0.25, "{out:?}");
    }

    #[test]
    fn hdr_is_refused_with_its_reason() {
        let error = Header::read(&fixture("pq.heic")).unwrap_err();
        assert_eq!(error, HeicError::Unsupported("HDR"));
        assert_eq!(error.to_string(), "Argentum can't open HDR HEIF photos yet");
    }

    #[test]
    fn a_damaged_file_is_a_typed_error_not_a_panic() {
        let whole = fixture("metadata.heic");
        for cut in 0..whole.len() {
            match decode(&whole[..cut]) {
                Err(HeicError::Corrupt(_) | HeicError::NoDecoder(_)) => {}
                other => panic!("cut at {cut}: {other:?}"),
            }
        }
        assert_eq!(
            decode(b"\0\0\0\x10ftypheic\0\0\0\0"),
            Err(HeicError::Corrupt("its header is damaged"))
        );
    }

    #[test]
    fn a_clean_aperture_crops_around_its_centre() {
        let clap = |width, height, h, v| {
            Property::CleanAperture(CleanAperture {
                width,
                height,
                horizontal_offset: h,
                vertical_offset: v,
            })
        };
        let crop_of = |p: Property, w, h| crop(&[&p], w, h);
        // libheif's 16x12 coded as 64x64: centred, offset half the difference.
        assert_eq!(
            crop_of(clap((16, 1), (12, 1), (-24, 1), (-26, 1)), 64, 64).unwrap(),
            Crop {
                left: 0,
                top: 0,
                width: 16,
                height: 12
            }
        );
        // A centre 2 left of the picture's.
        assert_eq!(
            crop_of(clap((8, 1), (8, 1), (-4, 2), (0, 1)), 16, 12).unwrap(),
            Crop {
                left: 2,
                top: 2,
                width: 8,
                height: 8
            }
        );
        assert!(crop_of(clap((17, 1), (12, 1), (0, 1), (0, 1)), 16, 12).is_err());
        assert!(crop_of(clap((0, 1), (8, 1), (0, 1), (0, 1)), 16, 12).is_err());
    }

    // The tests below need the system's HEIC decoder.

    #[test]
    fn a_heic_decodes_to_its_size_and_colours() {
        if !decoder_here() {
            return;
        }
        let image = decode(&fixture("metadata.heic")).unwrap();
        assert_eq!((image.width(), image.height()), (16, 12));
        assert_corners(&image, [RED, GREEN, WHITE, BLUE]);
        assert!(near(at(&image, 8, 6), [128; 3]), "{:?}", at(&image, 8, 6));
    }

    #[test]
    fn a_turned_heic_comes_out_upright() {
        if !decoder_here() {
            return;
        }
        // A quarter turn anticlockwise: green comes to the top left.
        let turned = decode(&fixture("rotated.heic")).unwrap();
        assert_eq!((turned.width(), turned.height()), (12, 16));
        assert_corners(&turned, [GREEN, WHITE, BLUE, RED]);
        // Then top and bottom swapped: the transpose.
        let mirrored = decode(&fixture("rotated-mirrored.heic")).unwrap();
        assert_eq!((mirrored.width(), mirrored.height()), (12, 16));
        assert_corners(&mirrored, [RED, BLUE, WHITE, GREEN]);
    }

    #[test]
    fn an_iphone_grid_and_ten_bits_decode() {
        if !decoder_here() {
            return;
        }
        let grid = decode(&fixture("grid.heic")).unwrap();
        assert_eq!((grid.width(), grid.height()), (120, 100));
        assert_corners(&grid, [RED, GREEN, WHITE, BLUE]);
        let deep = decode(&fixture("ten-bit.heic")).unwrap();
        assert_eq!((deep.width(), deep.height()), (16, 12));
        assert_corners(&deep, [RED, GREEN, WHITE, BLUE]);
    }

    /// p3.heic's corners are P3's own primaries and white, which convert to
    /// sRGB's (clipped): the decode keeps them where they are, the grey grey.
    /// How the conversion moves a colour inside both gamuts is tested above.
    #[test]
    fn a_display_p3_heic_keeps_its_colour() {
        if !decoder_here() {
            return;
        }
        let image = decode(&fixture("p3.heic")).unwrap();
        assert_corners(&image, [RED, GREEN, WHITE, BLUE]);
        assert!(near(at(&image, 8, 6), [128; 3]), "{:?}", at(&image, 8, 6));
    }

    /// Through the app's own decode, a HEIC opens as a JPEG does, and its
    /// EXIF reads as a JPEG's.
    #[test]
    fn the_app_opens_a_heic_with_its_exif() {
        let bytes = fixture("metadata.heic");
        let exif = crate::exif_processing::read_exif_data_from_bytes("x.heic", &bytes);
        assert_eq!(exif.get("Make").map(String::as_str), Some("Fixture"));
        assert_eq!(exif.get("Model").map(String::as_str), Some("Synthetic 1"));
        assert_eq!(
            exif.get("DateTimeOriginal").map(String::as_str),
            Some("2024:05:04 10:21:33")
        );
        if !decoder_here() {
            return;
        }
        let image = crate::image_loader::load_image_with_orientation(&bytes, None).unwrap();
        assert_eq!((image.width(), image.height()), (16, 12));
    }
}
