//! The crop mode a Canon body records in its maker note. Ours, on RAWmakase's
//! reading.
//!
//! A full-frame Canon in its 1.6x (APS-C) or 1.3x (APS-H) crop mode records only
//! that part of the sensor, and says so in the first value of its maker note's
//! AspectInfo (0x009a): 13 for APS-C, 12 for APS-H, as ExifTool names the codes
//! (RAWmakase `crates/rawmakase-raw/src/vendor/canon.rs::crop_mode` and
//! `canon_cr3.rs::sensor_crop` @ 1b2687c4, MIT). Canon writes no 35 mm
//! equivalent focal length, so this is the only place the crop mode shows.
//!
//! In a CR3 the maker note is a TIFF of its own, in the box named CMT3; in a
//! CR2 or a JPEG it is the MakerNote field of the EXIF. The crop mode goes in the EXIF
//! map under [`KEY`], as the crop mode's crop factor over the sensor's, only
//! when there is one; `lens_shot::MatchShot` reads it from there.

use std::collections::HashMap;

/// The EXIF map's key for a crop mode's crop factor over the sensor's.
pub const KEY: &str = "CropModeFactor";

/// Canon maker note AspectInfo.
const ASPECT_INFO: u16 = 0x009a;

/// The crop factor over the sensor's of AspectInfo's ratio `code`; `None` for
/// the whole sensor (the aspect ratios crop it, not its crop factor).
pub fn crop_mode(code: u32) -> Option<f32> {
    match code {
        12 => Some(1.3),
        13 => Some(1.6),
        _ => None,
    }
}

/// Add the crop mode of a Canon file to its EXIF map, when it was shot in one.
pub fn fill(map: &mut HashMap<String, String>, file_bytes: &[u8]) {
    let canon = map
        .get("Make")
        .is_some_and(|make| make.to_ascii_lowercase().contains("canon"));
    if !canon {
        return;
    }
    if let Some(factor) = read(file_bytes) {
        map.insert(KEY.to_string(), factor.to_string());
    }
}

/// How far into a CR3 to look for its maker note: it sits in the `moov` box,
/// in the first few kilobytes.
const CR3_SCAN: usize = 1 << 20;

/// The crop mode of a Canon file: AspectInfo's first value, from a CR3's maker
/// note box (`CMT3`, a TIFF of its own, as RAWmakase's `container/cr3.rs`
/// reads it) or a CR2's or JPEG's MakerNote field.
fn read(file_bytes: &[u8]) -> Option<f32> {
    let cr3 = || {
        let head = &file_bytes[..file_bytes.len().min(CR3_SCAN)];
        let at = head.windows(4).position(|w| w == b"CMT3")?;
        let notes = super::lens_name::tiff_block_at(file_bytes, at + 4)?;
        let field = notes
            .fields()
            .find(|f| f.tag.number() == ASPECT_INFO && f.ifd_num == exif::In::PRIMARY)?;
        match &field.value {
            exif::Value::Long(v) => v.first().copied(),
            _ => None,
        }
    };
    let cr2 = || {
        super::canon_makernote::longs(file_bytes, ASPECT_INFO, 1..=16)?
            .first()
            .copied()
    };
    crop_mode(cr3().or_else(cr2)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AspectInfo's crop modes, as ExifTool names them (RAWmakase's
    /// `crop_modes_name_their_crop_factor`).
    #[test]
    fn crop_modes_name_their_crop_factor() {
        assert_eq!(crop_mode(13), Some(1.6));
        assert_eq!(crop_mode(12), Some(1.3));
        for code in [0, 1, 2, 7] {
            assert_eq!(crop_mode(code), None);
        }
    }

    /// Another maker's file is never read as Canon's.
    #[test]
    fn only_a_canon_file_is_read() {
        let mut map = HashMap::from([("Make".to_string(), "NIKON CORPORATION".to_string())]);
        fill(&mut map, b"II*\0 not read");
        assert!(!map.contains_key(KEY));
    }

    /// A real crop-mode file: `AG_CANON_CROP` names a CR3 or CR2 shot in a crop
    /// mode (raw.pixls.us's `Canon_EOS_R5_CRAW_ISO_100_crop_nodual.CR3`), and
    /// `AG_CANON_FULL` optionally one shot on the whole sensor.
    #[test]
    #[ignore = "reads AG_CANON_CROP"]
    fn a_real_crop_mode_file() {
        let path = std::env::var("AG_CANON_CROP").expect("AG_CANON_CROP");
        let bytes = std::fs::read(&path).unwrap();
        let mut map = crate::exif_processing::read_exif_data_from_bytes(&path, &bytes);
        fill(&mut map, &bytes);
        println!(
            "{path}: {:?} {:?} {:?}",
            map.get("Model"),
            map.get("LensModel"),
            map.get(KEY)
        );
        assert_eq!(map.get(KEY).map(String::as_str), Some("1.6"));
        if let Ok(full) = std::env::var("AG_CANON_FULL") {
            let bytes = std::fs::read(&full).unwrap();
            let mut map = crate::exif_processing::read_exif_data_from_bytes(&full, &bytes);
            fill(&mut map, &bytes);
            assert_eq!(map.get(KEY), None, "{full}");
        }
    }
}
