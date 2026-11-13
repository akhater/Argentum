//! Apertures EXIF stores in APEX units, as the EXIF list shows them. Ours.
//!
//! `ApertureValue` and `MaxApertureValue` hold Av = 2 log2(N), not the
//! f-number N: f/5.6 is stored as about 5, f/16 as 8. The EXIF list prints
//! them as f-numbers, `f/5.6`, like `FNumber` and like exiftool's "Aperture
//! Value" and "Max Aperture Value". Exported files keep the stored value.

/// The f-number an APEX aperture value stands for, to one decimal as
/// exiftool prints it.
pub fn apex_to_f_number(av: f32) -> f32 {
    (2f32.powf(av / 2.0) * 10.0).round() / 10.0
}

/// An APEX aperture as the EXIF list shows it, `f/5.6`. None when it is not
/// a number (a rational over 0).
pub fn apex_aperture_text(av: f32) -> Option<String> {
    av.is_finite()
        .then(|| format!("f/{}", apex_to_f_number(av)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exif_processing::{
        extract_metadata, map_from_raw_metadata, read_exif_data_from_bytes,
    };
    use crate::mods::lens_shot::LensShot;
    use exif::experimental::Writer;
    use exif::{Field, In, Rational, Tag, Value};
    use rawler::decoders::RawMetadata;
    use std::io::Cursor;

    #[test]
    fn apex_values_read_as_exiftool_prints_them() {
        // Av 4.97 is what Canon and Sony write for f/5.6; exactly 5 is 5.66.
        for (av, f_number) in [
            (4.97, 5.6),
            (5.0, 5.7),
            (4.0, 4.0),
            (0.0, 1.0),
            (6.0, 8.0),
            (8.0, 16.0),
        ] {
            assert_eq!(apex_to_f_number(av), f_number, "Av {av}");
        }
        assert_eq!(apex_aperture_text(4.0).as_deref(), Some("f/4"));
        assert_eq!(apex_aperture_text(0.0).as_deref(), Some("f/1"));
        assert_eq!(apex_aperture_text(4.97).as_deref(), Some("f/5.6"));
        assert_eq!(apex_aperture_text(f32::NAN), None);
    }

    /// A TIFF holding these two APEX apertures, each `num/denom`.
    fn tiff_with(aperture: (u32, u32), max_aperture: (u32, u32)) -> Vec<u8> {
        let field = |tag, (num, denom)| Field {
            tag,
            ifd_num: In::PRIMARY,
            value: Value::Rational(vec![Rational { num, denom }]),
        };
        let fields = [
            field(Tag::ApertureValue, aperture),
            field(Tag::MaxApertureValue, max_aperture),
        ];
        let mut writer = Writer::new();
        fields.iter().for_each(|f| writer.push_field(f));
        let mut bytes = Cursor::new(Vec::new());
        writer.write(&mut bytes, true).expect("a TIFF");
        bytes.into_inner()
    }

    fn apertures(map: &std::collections::HashMap<String, String>) -> (&str, &str) {
        (
            map.get("ApertureValue").map_or("", String::as_str),
            map.get("MaxApertureValue").map_or("", String::as_str),
        )
    }

    #[test]
    fn a_raw_read_by_kamadak_lists_f_numbers() {
        for ((av, max), shown) in [
            (((497, 100), (4, 1)), ("f/5.6", "f/4")),
            (((5, 1), (0, 1)), ("f/5.7", "f/1")),
        ] {
            let map = extract_metadata(&tiff_with(av, max)).expect("a map");
            assert_eq!(apertures(&map), shown);
        }
    }

    #[test]
    fn a_jpeg_or_tiff_lists_f_numbers() {
        let map = read_exif_data_from_bytes("photo.tif", &tiff_with((497, 100), (4, 1)));
        assert_eq!(apertures(&map), ("f/5.6", "f/4"));
    }

    #[test]
    fn a_raw_read_by_rawler_lists_f_numbers() {
        use rawler::formats::tiff::Rational as RawlerRational;
        for ((av, max), shown) in [
            (((497, 100), (4, 1)), ("f/5.6", "f/4")),
            (((5, 1), (0, 1)), ("f/5.7", "f/1")),
        ] {
            let mut metadata = RawMetadata {
                exif: Default::default(),
                model: "Body".into(),
                make: "Maker".into(),
                lens: None,
                unique_image_id: None,
                rating: None,
            };
            metadata.exif.aperture_value = Some(RawlerRational::new(av.0, av.1));
            metadata.exif.max_aperture_value = Some(RawlerRational::new(max.0, max.1));
            assert_eq!(apertures(&map_from_raw_metadata(metadata)), shown);
        }
    }

    /// A rational over 0 is no aperture, not f/1.
    #[test]
    fn an_aperture_over_zero_is_not_listed_as_one() {
        let map = extract_metadata(&tiff_with((5, 0), (4, 1))).expect("a map");
        assert_ne!(map.get("ApertureValue").map(String::as_str), Some("f/1"));
    }

    /// Lens correction reads the list: a photo with only ApertureValue Av 8
    /// gets the f/16 profile.
    #[test]
    fn lens_correction_reads_the_listed_aperture() {
        let map = extract_metadata(&tiff_with((8, 1), (4, 1))).expect("a map");
        assert_eq!(LensShot::from_exif(&map).f_number, Some(16.0));
    }
}
