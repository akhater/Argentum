use rawler::bits::Endian;
use rawler::decoders::{Decoder, FormatHint, WellKnownIFD};
use rawler::formats::bmff::Bmff;
use rawler::formats::tiff::reader::TiffReader;
use rawler::formats::tiff::{ifd::OffsetMode, Value, IFD};
use rawler::tags::{DngTag, ExifTag};
use std::io::Cursor;
use std::panic::{catch_unwind, AssertUnwindSafe};

const HIGHLIGHT_BIAS_MIN: f32 = -1.0;
const HIGHLIGHT_BIAS_MAX: f32 = 4.0;

const CANON_LIGHTING_OPT: u16 = 0x4018;
const FUJI_DEVELOPMENT_DYNAMIC_RANGE: u16 = 0x1403;
const FUJI_AUTO_DYNAMIC_RANGE: u16 = 0x140b;
const NIKON_COLOR_SPACE: u16 = 0x001e;
const NIKON_ACTIVE_D_LIGHTING: u16 = 0x0022;
const OLYMPUS_CAMERA_SETTINGS: u16 = 0x2020;
const OLYMPUS_GRADATION: u16 = 0x050f;
const PENTAX_DYNAMIC_RANGE_EXPANSION: u16 = 0x0069;

const RAF_JPEG_OFFSET_POSITION: usize = 84;
const RAF_JPEG_TIFF_HEADER: u32 = 12;
const FUJI_MAKERNOTE_HEADER: &[u8] = b"FUJIFILM";
const FUJI_MAKERNOTE_IFD_OFFSET: u32 = 12;

#[derive(Clone, Copy, Debug, Default)]
pub struct RawExposure {
    pub scene_referred: bool,
    pub dng_baseline: f32,
    pub highlight_preservation: f32,
}

pub fn read(decoder: &dyn Decoder, file_bytes: &[u8], is_monochrome: bool) -> RawExposure {
    let highlight_preservation = catch_unwind(AssertUnwindSafe(|| {
        highlight_preservation(decoder.format_hint(), file_bytes)
    }))
    .unwrap_or(0.0);
    RawExposure {
        scene_referred: !is_monochrome,
        dng_baseline: dng_baseline(decoder),
        highlight_preservation: highlight_preservation
            .clamp(HIGHLIGHT_BIAS_MIN, HIGHLIGHT_BIAS_MAX),
    }
}

fn dng_baseline(decoder: &dyn Decoder) -> f32 {
    let Ok(Some(tags)) = decoder.ifd(WellKnownIFD::VirtualDngRootTags) else {
        return 0.0;
    };
    [DngTag::BaselineExposure, DngTag::BaselineExposureOffset]
        .into_iter()
        .filter_map(|tag| tags.get_entry(tag)?.value.get_f32(0).ok().flatten())
        .sum()
}

fn highlight_preservation(format: FormatHint, file_bytes: &[u8]) -> f32 {
    match format {
        FormatHint::CR2 => tiff_makernote(file_bytes).map(|makernote| canon(&makernote)),
        FormatHint::CR3 => Bmff::new_buf(file_bytes)
            .ok()
            .and_then(|bmff| bmff.filebox.moov.cr3desc)
            .map(|desc| canon(desc.cmt3.tiff.root_ifd())),
        FormatHint::NEF | FormatHint::NRW => {
            tiff_makernote(file_bytes).map(|makernote| nikon(&makernote))
        }
        FormatHint::PEF => tiff_makernote(file_bytes).map(|makernote| pentax(&makernote)),
        FormatHint::ORF => olympus_camera_settings(file_bytes).map(|settings| olympus(&settings)),
        FormatHint::RAF => fuji_makernote(file_bytes).map(|makernote| fuji(&makernote)),
        _ => None,
    }
    .unwrap_or(0.0)
}

fn exif_with_makernote(cursor: &mut Cursor<&[u8]>, tiff_offset: u32) -> Option<IFD> {
    IFD::new_root(cursor, tiff_offset)
        .ok()?
        .find_first_ifd_with_tag(ExifTag::MakerNotes)
        .cloned()
}

fn makernote_start(exif: &IFD) -> Option<u32> {
    exif.base
        .checked_add(exif.get_entry(ExifTag::MakerNotes)?.offset()? as u32)
}

fn tiff_makernote(file_bytes: &[u8]) -> Option<IFD> {
    let mut cursor = Cursor::new(file_bytes);
    exif_with_makernote(&mut cursor, 0)?
        .parse_makernote(&mut cursor, OffsetMode::Absolute, &[])
        .ok()?
}

fn fuji_makernote(file_bytes: &[u8]) -> Option<IFD> {
    let jpeg_offset = u32::from_be_bytes(
        file_bytes
            .get(RAF_JPEG_OFFSET_POSITION..RAF_JPEG_OFFSET_POSITION + 4)?
            .try_into()
            .ok()?,
    );
    let mut cursor = Cursor::new(file_bytes);
    let exif = exif_with_makernote(&mut cursor, jpeg_offset.checked_add(RAF_JPEG_TIFF_HEADER)?)?;
    if !matches!(
        exif.get_entry(ExifTag::MakerNotes).map(|entry| &entry.value),
        Some(Value::Undefined(data)) if data.starts_with(FUJI_MAKERNOTE_HEADER)
    ) {
        return None;
    }
    IFD::new(
        &mut cursor,
        FUJI_MAKERNOTE_IFD_OFFSET,
        makernote_start(&exif)?,
        0,
        Endian::Little,
        &[],
    )
    .ok()
}

fn olympus_camera_settings(file_bytes: &[u8]) -> Option<IFD> {
    let mut cursor = Cursor::new(file_bytes);
    let exif = exif_with_makernote(&mut cursor, 0)?;
    let makernote = rawler::decoders::orf::parse_makernote(&mut cursor, &exif).ok()??;
    let settings_offset = makernote
        .get_entry_raw_with_len(OLYMPUS_CAMERA_SETTINGS, &mut cursor, 4)
        .ok()??
        .get_force_u32(0);
    IFD::new(
        &mut cursor,
        settings_offset,
        makernote_start(&exif)?,
        0,
        makernote.endian,
        &[],
    )
    .ok()
}

/// One value of a tag, as i64, which holds every TIFF integer: rawler's
/// `get_i32` panics on a value above `i32::MAX` instead of failing, and
/// Canon's LightingOpt carries 4294967295 at index 13 on the EOS R6 Mark III,
/// R50, R10 and R50 V. Only the values used are read.
pub(crate) fn integer(ifd: &IFD, tag: u16, index: usize) -> Option<i64> {
    ifd.get_entry(tag)?.value.get_i64(index).ok().flatten()
}

fn canon(makernote: &IFD) -> f32 {
    let lighting_opt = |index| integer(makernote, CANON_LIGHTING_OPT, index);
    let auto_lighting_optimizer = match lighting_opt(2) {
        Some(0) => 0.5,
        Some(1) => 0.33,
        Some(2) => 0.66,
        _ => 0.0,
    };
    let highlight_tone_priority = match lighting_opt(3) {
        Some(state) if state > 0 => 1.0,
        _ => 0.0,
    };
    auto_lighting_optimizer + highlight_tone_priority
}

fn fuji(makernote: &IFD) -> f32 {
    match [FUJI_DEVELOPMENT_DYNAMIC_RANGE, FUJI_AUTO_DYNAMIC_RANGE]
        .into_iter()
        .find_map(|tag| integer(makernote, tag, 0))
    {
        Some(200) => 1.0,
        Some(400) => 2.0,
        _ => 0.0,
    }
}

fn nikon(makernote: &IFD) -> f32 {
    if integer(makernote, NIKON_COLOR_SPACE, 0) == Some(4) {
        return 2.0;
    }
    match integer(makernote, NIKON_ACTIVE_D_LIGHTING, 0) {
        Some(3) => 0.33,
        Some(5) => 0.66,
        Some(7) => 1.0,
        Some(8) => 1.1,
        Some(9) => 1.2,
        Some(10) => 1.3,
        Some(11) => 1.33,
        _ => 0.0,
    }
}

fn olympus(camera_settings: &IFD) -> f32 {
    match [0, 1, 2, 3].map(|index| integer(camera_settings, OLYMPUS_GRADATION, index)) {
        [_, _, _, Some(1)] => 0.33,
        [Some(-1), Some(-1), Some(1), _] => 0.66,
        _ => 0.0,
    }
}

fn pentax(makernote: &IFD) -> f32 {
    match makernote
        .get_entry(PENTAX_DYNAMIC_RANGE_EXPANSION)
        .map(|entry| &entry.value)
    {
        Some(Value::Undefined(data)) if data.first() == Some(&1) => 1.0,
        _ => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rawler::formats::tiff::Entry;

    /// Canon's LightingOpt as the EOS R6 Mark III, R50, R10 and R50 V write
    /// it: 4294967295 at index 13, beside the settings read here.
    fn canon_makernote(auto_lighting_optimizer: u32, highlight_tone_priority: u32) -> IFD {
        let mut lighting_opt = vec![56, 0, auto_lighting_optimizer, highlight_tone_priority];
        lighting_opt.extend([0; 9]);
        lighting_opt.push(u32::MAX);
        let mut makernote = IFD::default();
        makernote.entries.insert(
            CANON_LIGHTING_OPT,
            Entry {
                tag: CANON_LIGHTING_OPT,
                value: Value::Long(lighting_opt),
                embedded: None,
            },
        );
        makernote
    }

    const ALO_OFF: u32 = 3;
    const ALO_STANDARD: u32 = 0;
    const HTP_OFF: u32 = 0;
    const HTP_ON: u32 = 1;

    #[test]
    fn a_value_too_big_for_i32_does_not_hide_highlight_tone_priority() {
        assert_eq!(canon(&canon_makernote(ALO_OFF, HTP_ON)), 1.0);
        assert_eq!(canon(&canon_makernote(ALO_STANDARD, HTP_OFF)), 0.5);
    }

    #[test]
    fn highlight_tone_priority_holds_back_a_stop() {
        assert_eq!(canon(&canon_makernote(ALO_OFF, HTP_ON)), 1.0);
    }

    /// A Fujifilm JPEG offset of u32::MAX: the TIFF header added to it
    /// overflows. The file has no such data, so nothing is added.
    #[test]
    fn a_raf_jpeg_offset_at_the_top_of_u32_adds_nothing() {
        let mut bytes = vec![0u8; 96];
        bytes[RAF_JPEG_OFFSET_POSITION..RAF_JPEG_OFFSET_POSITION + 4]
            .copy_from_slice(&u32::MAX.to_be_bytes());
        assert_eq!(highlight_preservation(FormatHint::RAF, &bytes), 0.0);
    }

    /// A maker note whose start (the TIFF base plus its offset) overflows u32.
    #[test]
    fn a_maker_note_start_past_the_top_of_u32_is_not_there() {
        const MAKER_NOTES: u16 = 0x927C;
        let mut exif = IFD {
            base: u32::MAX - 4,
            ..Default::default()
        };
        exif.entries.insert(
            MAKER_NOTES,
            Entry {
                tag: MAKER_NOTES,
                value: Value::Undefined(vec![0; 4]),
                embedded: Some(16),
            },
        );
        assert_eq!(makernote_start(&exif), None);
    }
}
