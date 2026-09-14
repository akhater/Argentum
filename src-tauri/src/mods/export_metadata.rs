//! EXIF for formats upstream's writer turns away.
//!
//! Upstream gathers metadata beautifully — `write_image_with_metadata` reads a
//! source's full EXIF block, falls back to our sidecar, falls back again to
//! rawler for a RAW, and applies GPS. What it cannot do is put the result into
//! a TIFF, and it says so:
//!
//! ```text
//! // FIXME: temporary solution until I find a way to write metadata to TIFF
//! ```
//!
//! The reason is structural rather than missing support. `little_exif` treats a
//! TIFF as one big EXIF structure: writing to one *replaces the whole file*
//! with an encoding of the `Metadata`, strip data and all. Upstream builds a
//! fresh `Metadata::new()` — right for JPEG, where metadata is a segment spliced
//! into a file that already exists — and a fresh one holds no strips, no
//! dimensions and none of the ten structural tags a TIFF is required to carry.
//! It fails the library's own check and the write is dropped with a warning.
//!
//! So we do the opposite: **seed from the TIFF we just encoded**, which already
//! has the strips and the structural tags, and merge upstream's tags into that.
//!
//! Getting upstream's tags without reimplementing 250 lines of tag copying, and
//! without editing their file, is what the carrier is for: we hand their own
//! function a one-pixel JPEG, let it do its work, and read the metadata back
//! out of the result. Their gathering stays theirs, bugs, fixes and all.
//!
//! Everything that is not a TIFF is passed straight through to them.

use std::io::Cursor;

use image::{DynamicImage, ImageBuffer, Rgb};
use little_exif::exif_tag::ExifTag;
use little_exif::filetype::FileExtension;
use little_exif::metadata::Metadata;

/// Tags that describe the *file we just wrote*, never the photograph it came
/// from.
///
/// The carrier's source may legitimately carry any of these — a JPEG written by
/// Photoshop or a phone routinely has `ImageWidth`/`ImageLength` in IFD0 — and
/// `set_tag` replaces by tag number. Copy them across and the exported TIFF
/// inherits the *source's* dimensions, strip layout and bit depth: a file that
/// describes an image nobody wrote. A resized export becomes unreadable.
///
/// So these stay as our encoder wrote them, always.
const STRUCTURAL: &[u16] = &[
    0x0100, // ImageWidth
    0x0101, // ImageLength
    0x0102, // BitsPerSample
    0x0103, // Compression
    0x0106, // PhotometricInterpretation
    0x0111, // StripOffsets — carries the pixel data itself
    0x0115, // SamplesPerPixel
    0x0116, // RowsPerStrip
    0x0117, // StripByteCounts
    0x011A, // XResolution
    0x011B, // YResolution
    0x011C, // PlanarConfiguration
    0x0128, // ResolutionUnit
    0x013D, // Predictor
    0x0153, // SampleFormat
    0x8769, // ExifOffset — a pointer; `encode()` writes its own
    0x8825, // GPSInfo — likewise
];

/// True for the two spellings of TIFF.
///
/// Both reach the encoder: `encode_image_to_bytes` matches `"tif" | "tiff"`.
/// Matching only the longer one here is how a `.tif` export would quietly lose
/// its metadata while a `.tiff` kept it.
fn is_tiff(format: &str) -> bool {
    matches!(format, "tif" | "tiff")
}

/// Attach metadata to an encoded image, whatever the format.
///
/// Upstream's function for everything it handles; ours for TIFF. The signature
/// matches theirs so the call site reads the same.
pub fn write_export_metadata(
    image_bytes: &mut Vec<u8>,
    original_path_str: &str,
    output_format: &str,
    keep_metadata: bool,
    strip_gps: bool,
) -> Result<(), String> {
    let format = output_format.to_lowercase();

    if !keep_metadata || !is_tiff(&format) {
        return crate::exif_processing::write_image_with_metadata(
            image_bytes,
            original_path_str,
            output_format,
            keep_metadata,
            strip_gps,
        );
    }

    // A failure here means a TIFF without metadata, which is what a TIFF got
    // before this module existed. It is never a reason to fail the export and
    // lose the photograph, so every path below warns and returns Ok.
    match merge_into_tiff(image_bytes, original_path_str, strip_gps) {
        Ok(()) => Ok(()),
        Err(e) => {
            log::warn!("Could not write metadata into the exported TIFF: {e}");
            Ok(())
        }
    }
}

/// Upstream's gathered tags, merged into the TIFF we encoded.
fn merge_into_tiff(
    image_bytes: &mut Vec<u8>,
    original_path_str: &str,
    strip_gps: bool,
) -> Result<(), String> {
    let carrier = gather_via_carrier(original_path_str, strip_gps)?;

    // Seed from our own output: this is what holds the strips and the ten
    // structural tags `little_exif` insists on before it will write a TIFF.
    let mut metadata = Metadata::new_from_vec(image_bytes, FileExtension::TIFF)
        .map_err(|e| format!("could not read back the TIFF we just encoded: {e}"))?;

    for tag in &carrier {
        if STRUCTURAL.contains(&tag.as_u16()) || !tag.is_writable() {
            continue;
        }
        metadata.set_tag(tag.clone());
    }

    // The carrier is one pixel, so whatever upstream put in these is wrong for
    // this file. They are the photograph's dimensions, not the file's, but a
    // reader that trusts them deserves the truth.
    if let Ok(reader) =
        image::ImageReader::new(Cursor::new(image_bytes.as_slice())).with_guessed_format()
        && let Ok((width, height)) = reader.into_dimensions()
    {
        metadata.set_tag(ExifTag::ExifImageWidth(vec![width]));
        metadata.set_tag(ExifTag::ExifImageHeight(vec![height]));
    }

    metadata.set_tag(ExifTag::Software("Argentum".to_string()));

    metadata
        .write_to_vec(image_bytes, FileExtension::TIFF)
        .map_err(|e| format!("little_exif refused the TIFF: {e}"))?;

    Ok(())
}

/// Upstream's tag gathering, run against a throwaway one-pixel JPEG.
///
/// This is the whole trick. `write_image_with_metadata` does not expose the
/// `Metadata` it builds — it writes it and returns `()` — so the only way to
/// reuse it without copying it into our file is to give it something to write
/// *into* and read the result back. A 1x1 JPEG costs a few hundred bytes and
/// gives us every tag they gathered: full EXIF from a non-RAW source, the
/// `.agexif` sidecar, rawler's block for a RAW, GPS, and any fix they make
/// later without us noticing.
fn gather_via_carrier(original_path_str: &str, strip_gps: bool) -> Result<Metadata, String> {
    let pixel: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::from_pixel(1, 1, Rgb([0, 0, 0]));
    let mut carrier = Vec::new();
    DynamicImage::ImageRgb8(pixel)
        .write_to(&mut Cursor::new(&mut carrier), image::ImageFormat::Jpeg)
        .map_err(|e| format!("could not build the carrier: {e}"))?;

    crate::exif_processing::write_image_with_metadata(
        &mut carrier,
        original_path_str,
        "jpeg",
        true,
        strip_gps,
    )?;

    Metadata::new_from_vec(&carrier, FileExtension::JPEG)
        .map_err(|e| format!("carrier carried nothing back: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::GenericImageView;
    use std::path::{Path, PathBuf};

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("argentum-export-metadata-tests");
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    fn rgb16_image(w: u32, h: u32) -> DynamicImage {
        DynamicImage::ImageRgb16(ImageBuffer::from_fn(w, h, |x, y| {
            Rgb([(x * 997) as u16, (y * 1031) as u16, (x * y) as u16])
        }))
    }

    fn rgb8_image(w: u32, h: u32) -> DynamicImage {
        DynamicImage::ImageRgb8(ImageBuffer::from_fn(w, h, |x, y| {
            Rgb([(x % 251) as u8, (y % 241) as u8, ((x + y) % 239) as u8])
        }))
    }

    fn encode(image: &DynamicImage) -> Vec<u8> {
        let mut bytes = Vec::new();
        crate::mods::export_precision::encode_tiff(image, &mut Cursor::new(&mut bytes)).unwrap();
        bytes
    }

    /// A JPEG on disk to export *from*, carrying the tags a camera would.
    fn source_jpeg(name: &str, extra: &[ExifTag]) -> PathBuf {
        let path = scratch(name);
        let mut bytes = Vec::new();
        DynamicImage::ImageRgb8(ImageBuffer::from_pixel(8, 8, Rgb([9u8, 9, 9])))
            .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Jpeg)
            .unwrap();

        let mut metadata = Metadata::new();
        metadata.set_tag(ExifTag::Make("TestCam Industries".to_string()));
        metadata.set_tag(ExifTag::Model("TC-1".to_string()));
        for tag in extra {
            metadata.set_tag(tag.clone());
        }
        metadata
            .write_to_vec(&mut bytes, FileExtension::JPEG)
            .unwrap();

        std::fs::write(&path, &bytes).unwrap();
        path
    }

    fn read_back(bytes: &[u8]) -> exif::Exif {
        exif::Reader::new()
            .read_from_container(&mut Cursor::new(bytes))
            .expect("exported TIFF carries no readable EXIF")
    }

    /// A field as a plain string. `display_value` quotes ASCII tags; strip that
    /// so a test reads like the value a person would see in exiftool.
    fn field(exif: &exif::Exif, tag: exif::Tag) -> Option<String> {
        exif.get_field(tag, exif::In::PRIMARY)
            .map(|f| f.display_value().to_string().trim_matches('"').to_string())
    }

    /// The photograph must survive intact. Everything else is decoration.
    fn assert_image_unchanged(original: &DynamicImage, exported: &[u8], label: &str) {
        let decoded = image::load_from_memory(exported)
            .unwrap_or_else(|e| panic!("{label}: exported TIFF no longer decodes: {e}"));
        assert_eq!(
            decoded.dimensions(),
            original.dimensions(),
            "{label}: dimensions"
        );
        assert_eq!(
            decoded.color(),
            original.color(),
            "{label}: colour type / bit depth"
        );
        assert_eq!(
            decoded.as_bytes(),
            original.as_bytes(),
            "{label}: pixel data"
        );
    }

    #[test]
    fn a_16_bit_tiff_keeps_its_pixels_and_gains_its_exif() {
        let source = source_jpeg("source-16.jpg", &[]);
        let original = rgb16_image(64, 48);
        let mut bytes = encode(&original);

        write_export_metadata(&mut bytes, source.to_str().unwrap(), "tiff", true, true).unwrap();

        assert_image_unchanged(&original, &bytes, "rgb16");
        let exif = read_back(&bytes);
        assert_eq!(
            field(&exif, exif::Tag::Make).as_deref(),
            Some("TestCam Industries")
        );
        assert_eq!(field(&exif, exif::Tag::Model).as_deref(), Some("TC-1"));
    }

    #[test]
    fn an_8_bit_tiff_keeps_its_pixels_and_gains_its_exif() {
        let source = source_jpeg("source-8.jpg", &[]);
        let original = rgb8_image(64, 48);
        let mut bytes = encode(&original);

        write_export_metadata(&mut bytes, source.to_str().unwrap(), "tiff", true, true).unwrap();

        assert_image_unchanged(&original, &bytes, "rgb8");
        assert_eq!(
            field(&read_back(&bytes), exif::Tag::Make).as_deref(),
            Some("TestCam Industries")
        );
    }

    /// The bug this module would have shipped without [`STRUCTURAL`].
    ///
    /// A source JPEG written by Photoshop or a phone carries `ImageWidth` and
    /// `ImageLength` in IFD0. `set_tag` replaces by tag number, so copying them
    /// across stamps the *source's* dimensions onto our TIFF, and a reader that
    /// believes the header finds an image of the wrong size.
    #[test]
    fn a_sources_own_dimensions_never_reach_the_exported_tiff() {
        let source = source_jpeg(
            "source-liar.jpg",
            &[
                ExifTag::ImageWidth(vec![9999]),
                ExifTag::ImageHeight(vec![7777]),
            ],
        );
        let original = rgb16_image(64, 48);
        let mut bytes = encode(&original);

        write_export_metadata(&mut bytes, source.to_str().unwrap(), "tiff", true, true).unwrap();

        assert_image_unchanged(&original, &bytes, "structural guard");

        let exif = read_back(&bytes);
        assert_eq!(
            field(&exif, exif::Tag::ImageWidth).as_deref(),
            Some("64"),
            "the source's ImageWidth overwrote ours"
        );
        assert_eq!(
            field(&exif, exif::Tag::ImageLength).as_deref(),
            Some("48"),
            "the source's ImageLength overwrote ours"
        );
        // Gathering still worked - the guard is selective, not a blanket skip.
        assert_eq!(
            field(&exif, exif::Tag::Make).as_deref(),
            Some("TestCam Industries")
        );
    }

    /// Both spellings reach the encoder, so both must reach us.
    #[test]
    fn tif_is_spelled_two_ways_and_both_get_metadata() {
        let source = source_jpeg("source-tif.jpg", &[]);
        let original = rgb8_image(32, 24);

        for format in ["tif", "tiff", "TIFF", "Tif"] {
            let mut bytes = encode(&original);
            write_export_metadata(&mut bytes, source.to_str().unwrap(), format, true, true)
                .unwrap();
            assert_image_unchanged(&original, &bytes, format);
            assert_eq!(
                field(&read_back(&bytes), exif::Tag::Make).as_deref(),
                Some("TestCam Industries"),
                "{format} got no metadata"
            );
        }
    }

    /// Off means off: the bytes must come back exactly as the encoder wrote them.
    #[test]
    fn keep_metadata_off_leaves_the_file_byte_for_byte() {
        let source = source_jpeg("source-off.jpg", &[]);
        let original = rgb16_image(32, 24);
        let encoded = encode(&original);

        let mut bytes = encoded.clone();
        write_export_metadata(&mut bytes, source.to_str().unwrap(), "tiff", false, true).unwrap();

        assert_eq!(bytes, encoded, "a TIFF was rewritten with the toggle off");
    }

    /// A missing source is a warning, not a failed export.
    #[test]
    fn an_unreadable_source_still_exports_the_photograph() {
        let original = rgb8_image(16, 16);
        let mut bytes = encode(&original);
        let missing = scratch("does-not-exist.jpg");
        assert!(!Path::new(&missing).exists());

        write_export_metadata(&mut bytes, missing.to_str().unwrap(), "tiff", true, true).unwrap();

        assert_image_unchanged(&original, &bytes, "missing source");
    }

    /// Anything that is not a TIFF is upstream's, unchanged.
    #[test]
    fn a_jpeg_export_still_goes_through_upstream() {
        let source = source_jpeg("source-jpeg-path.jpg", &[]);
        let mut ours = Vec::new();
        DynamicImage::ImageRgb8(ImageBuffer::from_pixel(16, 16, Rgb([3u8, 4, 5])))
            .write_to(&mut Cursor::new(&mut ours), image::ImageFormat::Jpeg)
            .unwrap();
        let mut theirs = ours.clone();

        write_export_metadata(&mut ours, source.to_str().unwrap(), "jpeg", true, true).unwrap();
        crate::exif_processing::write_image_with_metadata(
            &mut theirs,
            source.to_str().unwrap(),
            "jpeg",
            true,
            true,
        )
        .unwrap();

        assert_eq!(ours, theirs, "we changed what a JPEG export produces");
    }
}
