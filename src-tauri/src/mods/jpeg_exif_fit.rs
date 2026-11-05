//! A JPEG export's EXIF, trimmed to fit the one APP1 segment that carries it.
//!
//! A JPEG segment's length is a 16-bit number, so its EXIF can hold at most
//! [`MAX_EXIF_BYTES`]. little_exif 0.6.23 writes the length with `as u16`
//! (`jpg.rs`, `encode_metadata_jpg`) and never checks: a source with more
//! EXIF than that gives a length that wraps, readers lose the segments after
//! it, and the export still reports success. A camera's own EXIF rarely gets
//! there; a JPEG or PNG source whose EXIF was copied whole can.
//!
//! The rule is RAWmakase's (`crates/rawmakase-export/src/export/metadata.rs`
//! @ 23504b93, MIT): drop the largest values first, the EXIF directory's
//! before the main one's, and never what says where, when and with what the
//! photo was taken. GPS is never dropped either: its directory is small, and
//! the export already removes it when asked to.

use little_exif::ifd::ExifTagGroup;
use little_exif::metadata::Metadata;

/// The most EXIF bytes one APP1 segment holds: 65535 less the two bytes of
/// the length itself and the six of `Exif\0\0`.
pub const MAX_EXIF_BYTES: usize = 65535 - 2 - 6;

/// Tags that stay whatever their size: the camera, lens, capture time,
/// exposure, credit and what the export itself sets (`write_exif_metadata`).
const KEPT: &[(u16, ExifTagGroup)] = &[
    (0x010f, ExifTagGroup::GENERIC), // Make
    (0x0110, ExifTagGroup::GENERIC), // Model
    (0x0112, ExifTagGroup::GENERIC), // Orientation
    (0x011a, ExifTagGroup::GENERIC), // XResolution
    (0x011b, ExifTagGroup::GENERIC), // YResolution
    (0x0128, ExifTagGroup::GENERIC), // ResolutionUnit
    (0x0131, ExifTagGroup::GENERIC), // Software
    (0x013b, ExifTagGroup::GENERIC), // Artist
    (0x8298, ExifTagGroup::GENERIC), // Copyright
    (0x829a, ExifTagGroup::EXIF),    // ExposureTime
    (0x829d, ExifTagGroup::EXIF),    // FNumber
    (0x8827, ExifTagGroup::EXIF),    // ISO
    (0x9000, ExifTagGroup::EXIF),    // ExifVersion
    (0x9003, ExifTagGroup::EXIF),    // DateTimeOriginal
    (0x9004, ExifTagGroup::EXIF),    // CreateDate
    (0x9010, ExifTagGroup::EXIF),    // OffsetTime
    (0x9011, ExifTagGroup::EXIF),    // OffsetTimeOriginal
    (0x9012, ExifTagGroup::EXIF),    // OffsetTimeDigitized
    (0x9101, ExifTagGroup::EXIF),    // ComponentsConfiguration
    (0x9204, ExifTagGroup::EXIF),    // ExposureCompensation
    (0x920a, ExifTagGroup::EXIF),    // FocalLength
    (0xa000, ExifTagGroup::EXIF),    // FlashpixVersion
    (0xa001, ExifTagGroup::EXIF),    // ColorSpace
    (0xa002, ExifTagGroup::EXIF),    // ExifImageWidth
    (0xa003, ExifTagGroup::EXIF),    // ExifImageHeight
    (0xa405, ExifTagGroup::EXIF),    // FocalLengthIn35mmFormat
    (0xa433, ExifTagGroup::EXIF),    // LensMake
    (0xa434, ExifTagGroup::EXIF),    // LensModel
];

/// The size `metadata` encodes to, as a JPEG's APP1 payload.
fn encoded_bytes(metadata: &Metadata) -> Option<usize> {
    metadata.encode().ok().map(|bytes| bytes.len())
}

/// The tag to drop next: the largest value outside [`KEPT`] and GPS, the EXIF
/// and Interop directories' before the main one's.
fn next_to_drop(metadata: &Metadata) -> Option<(u16, ExifTagGroup)> {
    let endian = metadata.get_endian();
    metadata
        .get_ifds()
        .iter()
        .filter(|ifd| ifd.get_generic_ifd_nr() == 0 && ifd.get_ifd_type() != ExifTagGroup::GPS)
        .flat_map(|ifd| {
            let group = ifd.get_ifd_type();
            ifd.get_tags().iter().map(move |tag| (tag, group))
        })
        .filter(|(tag, group)| !KEPT.contains(&(tag.as_u16(), *group)))
        .max_by_key(|(tag, group)| {
            let main = *group == ExifTagGroup::GENERIC;
            (!main, tag.value_as_u8_vec(&endian).len())
        })
        .map(|(tag, group)| (tag.as_u16(), group))
}

/// Trims `metadata` until it fits one APP1 segment, and returns how many tags
/// were dropped. EXIF that still does not fit with only [`KEPT`] tags left is
/// cleared, so the export writes none rather than a broken file.
pub fn fit_one_segment(metadata: &mut Metadata) -> usize {
    let mut dropped = 0;
    loop {
        match encoded_bytes(metadata) {
            Some(size) if size <= MAX_EXIF_BYTES => return dropped,
            None => return dropped,
            Some(_) => {}
        }
        let Some((hex, group)) = next_to_drop(metadata) else {
            *metadata = Metadata::new();
            return dropped;
        };
        dropped += metadata.remove_tag_by_hex_group(hex, group).max(1);
    }
}

/// `metadata`, if it is meant for a JPEG, trimmed with [`fit_one_segment`],
/// saying so in the log when anything was dropped.
pub fn fit_for_jpeg(metadata: &mut Metadata, source: &str) {
    let dropped = fit_one_segment(metadata);
    if dropped > 0 {
        log::warn!(
            "The EXIF of {source} is larger than a JPEG segment holds: dropped {dropped} of its largest fields"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use little_exif::exif_tag::ExifTag;
    use little_exif::filetype::FileExtension;
    use std::io::Cursor;

    fn tag_present(metadata: &Metadata, tag: &ExifTag) -> bool {
        metadata.get_tag(tag).next().is_some()
    }

    fn camera() -> Metadata {
        let mut metadata = Metadata::new();
        metadata.set_tag(ExifTag::Make("Canon".into()));
        metadata.set_tag(ExifTag::Model("Canon EOS R6m3".into()));
        metadata.set_tag(ExifTag::DateTimeOriginal("2026:11:05 10:00:00".into()));
        metadata.set_tag(ExifTag::LensModel("RF24-105mm F4 L IS USM".into()));
        metadata
    }

    fn small_jpeg() -> Vec<u8> {
        let mut bytes = Vec::new();
        image::DynamicImage::new_rgb8(8, 8)
            .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Jpeg)
            .unwrap();
        bytes
    }

    #[test]
    fn exif_that_fits_is_left_alone() {
        let mut metadata = camera();
        metadata.set_tag(ExifTag::UserComment(vec![b'x'; 4000]));
        let before = metadata.encode().unwrap();
        assert_eq!(fit_one_segment(&mut metadata), 0);
        assert_eq!(metadata.encode().unwrap(), before);
    }

    #[test]
    fn the_largest_values_go_first_and_the_camera_stays() {
        let mut metadata = camera();
        metadata.set_tag(ExifTag::MakerNote(vec![7; 70_000]));
        metadata.set_tag(ExifTag::UserComment(vec![b'x'; 1000]));
        assert!(metadata.encode().unwrap().len() > MAX_EXIF_BYTES);

        assert_eq!(fit_one_segment(&mut metadata), 1);
        assert!(metadata.encode().unwrap().len() <= MAX_EXIF_BYTES);
        assert!(!tag_present(&metadata, &ExifTag::MakerNote(Vec::new())));
        assert!(tag_present(&metadata, &ExifTag::UserComment(Vec::new())));
        for tag in [
            ExifTag::Make(String::new()),
            ExifTag::Model(String::new()),
            ExifTag::DateTimeOriginal(String::new()),
            ExifTag::LensModel(String::new()),
        ] {
            assert!(tag_present(&metadata, &tag), "{tag:?} was dropped");
        }
    }

    #[test]
    fn the_exif_directory_goes_before_the_main_one() {
        let mut metadata = camera();
        // The main directory's value is the larger, and still waits its turn.
        metadata.set_tag(ExifTag::ImageDescription("d".repeat(40_000)));
        metadata.set_tag(ExifTag::UserComment(vec![b'x'; 30_000]));
        assert!(metadata.encode().unwrap().len() > MAX_EXIF_BYTES);

        fit_one_segment(&mut metadata);
        assert!(!tag_present(&metadata, &ExifTag::UserComment(Vec::new())));
        assert!(tag_present(
            &metadata,
            &ExifTag::ImageDescription(String::new())
        ));
    }

    #[test]
    fn exif_too_large_even_when_trimmed_is_not_written() {
        let mut metadata = Metadata::new();
        metadata.set_tag(ExifTag::Make("M".repeat(70_000)));
        fit_one_segment(&mut metadata);
        assert!(
            metadata
                .get_ifds()
                .iter()
                .all(|ifd| ifd.get_tags().is_empty())
        );
    }

    /// The fault itself: twenty 4000-byte values, as in RAWmakase's report
    /// (#774), written into a JPEG that must still decode with its camera.
    #[test]
    fn a_trimmed_export_is_a_readable_jpeg() {
        let mut metadata = camera();
        for i in 0..20u16 {
            metadata.set_tag(ExifTag::UnknownUNDEF(
                vec![b'a'; 4000],
                0xc000 + i,
                ExifTagGroup::EXIF,
            ));
        }
        fit_one_segment(&mut metadata);

        let mut jpeg = small_jpeg();
        metadata
            .write_to_vec(&mut jpeg, FileExtension::JPEG)
            .unwrap();
        assert!(image::load_from_memory(&jpeg).is_ok());
        let exif = exif::Reader::new()
            .read_from_container(&mut Cursor::new(&jpeg))
            .unwrap();
        let make = exif.get_field(exif::Tag::Make, exif::In::PRIMARY).unwrap();
        assert_eq!(make.display_value().to_string(), "\"Canon\"");
    }
}
