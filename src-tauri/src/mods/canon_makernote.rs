//! Reading a SHORT array straight out of the Canon MakerNote.
//!
//! Two decode fixes need this â€” `sraw_levels` wants ColorData (0x4001), and
//! `canon_old_wb` wants the old white balance table (0x00a4) â€” and rawler
//! exposes neither. The route is the same for both, so it lives once.
//!
//! Standard EXIF hands us the MakerNote as an undefined blob. On Canon that
//! blob is a bare IFD, and its value offsets are relative to the start of the
//! *file*, not to the blob. That is why every call needs the whole file as well
//! as the blob.
//!
//! BYTE ORDER IS NOT A CONSTANT
//!
//! This began life inside `sraw_levels`, reading CR2, and CR2 is always `II`.
//! So it read little-endian and said so in a comment. The EOS-1Ds writes `MM` —
//! its raw is a big-endian TIFF from before the CR2 container existed — and a
//! big-endian IFD read little-endian does not fail: the entry count comes back
//! as some huge number, the walk finds nothing, and the answer is a polite
//! `None`. It cost a whole render cycle to notice, because "no white balance
//! found" is exactly the symptom the fix was for.
//!
//! The IFD inside a MakerNote uses the container's byte order, so it is read
//! from the file's own TIFF header.

/// TIFF field type 3: SHORT.
pub const TYPE_SHORT: u16 = 3;

/// The byte order of a TIFF container, and of every IFD inside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Endian {
    Little,
    Big,
}

impl Endian {
    /// From the two magic bytes a TIFF, CR2 or old Canon .TIF starts with.
    pub fn of(file_bytes: &[u8]) -> Option<Self> {
        match file_bytes.get(..2)? {
            b"II" => Some(Self::Little),
            b"MM" => Some(Self::Big),
            _ => None,
        }
    }

    fn u16(self, b: [u8; 2]) -> u16 {
        match self {
            Self::Little => u16::from_le_bytes(b),
            Self::Big => u16::from_be_bytes(b),
        }
    }

    fn u32(self, b: [u8; 4]) -> u32 {
        match self {
            Self::Little => u32::from_le_bytes(b),
            Self::Big => u32::from_be_bytes(b),
        }
    }
}

/// Sanity bound on the MakerNote IFD, as in `makernote_lens`.
const MAX_ENTRIES: u16 = 512;

/// The SHORT array at `tag`, if the file is a Canon and carries one.
///
/// `count` is what the caller is prepared to believe about the array's length.
/// A count outside it is treated as "this is not the tag I meant" rather than
/// trusted, because a wrong offset read as an array is silent nonsense.
pub fn shorts(
    file_bytes: &[u8],
    tag: u16,
    count: std::ops::RangeInclusive<usize>,
) -> Option<Vec<u16>> {
    let mut cursor = std::io::Cursor::new(file_bytes);
    let exif = exif::Reader::new().read_from_container(&mut cursor).ok()?;

    let make = exif
        .get_field(exif::Tag::Make, exif::In::PRIMARY)
        .map(|f| f.display_value().to_string())?;
    if !make.to_ascii_lowercase().contains("canon") {
        return None;
    }

    let field = exif.get_field(exif::Tag::MakerNote, exif::In::PRIMARY)?;
    let blob = match &field.value {
        exif::Value::Undefined(bytes, _) => bytes.as_slice(),
        _ => return None,
    };
    // A container with no TIFF magic — CR3 is ISO-BMFF, not TIFF — is read the
    // way this always read everything, so nothing that worked before can stop
    // working because the byte order became a question.
    let endian = Endian::of(file_bytes).unwrap_or(Endian::Little);
    parse(blob, file_bytes, tag, count, endian)
}

/// Walk the IFD in `blob` for `tag` and decode its SHORT array out of the file.
pub fn parse(
    blob: &[u8],
    file_bytes: &[u8],
    tag: u16,
    count: std::ops::RangeInclusive<usize>,
    endian: Endian,
) -> Option<Vec<u16>> {
    let u16_at = |i: usize| -> Option<u16> { Some(endian.u16([*blob.get(i)?, *blob.get(i + 1)?])) };
    let u32_at = |i: usize| -> Option<u32> {
        Some(endian.u32([
            *blob.get(i)?,
            *blob.get(i + 1)?,
            *blob.get(i + 2)?,
            *blob.get(i + 3)?,
        ]))
    };

    let entries = u16_at(0)?;
    if entries == 0 || entries > MAX_ENTRIES {
        return None;
    }

    for i in 0..entries as usize {
        let entry = 2 + i * 12;
        if u16_at(entry)? != tag {
            continue;
        }
        if u16_at(entry + 2)? != TYPE_SHORT {
            return None;
        }
        let n = u32_at(entry + 4)? as usize;
        if !count.contains(&n) {
            return None;
        }
        let at = u32_at(entry + 8)? as usize;
        let bytes = file_bytes.get(at..at.checked_add(n * 2)?)?;
        return Some(
            bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| endian.u16([b[0], b[1]]))
                .collect(),
        );
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const TAG: u16 = 0x4001;

    /// Build an IFD with one entry pointing at a SHORT array in a file, in
    /// whichever byte order is being tested.
    fn one_entry_ifd(tag: u16, values: &[u16], at: u32, endian: Endian) -> (Vec<u8>, Vec<u8>) {
        let u16b = |v: u16| match endian {
            Endian::Little => v.to_le_bytes(),
            Endian::Big => v.to_be_bytes(),
        };
        let u32b = |v: u32| match endian {
            Endian::Little => v.to_le_bytes(),
            Endian::Big => v.to_be_bytes(),
        };

        let mut file = vec![0u8; at as usize];
        for v in values {
            file.extend_from_slice(&u16b(*v));
        }
        let mut blob = vec![0u8; 2 + 12];
        blob[0..2].copy_from_slice(&u16b(1));
        blob[2..4].copy_from_slice(&u16b(tag));
        blob[4..6].copy_from_slice(&u16b(TYPE_SHORT));
        blob[6..10].copy_from_slice(&u32b(values.len() as u32));
        blob[10..14].copy_from_slice(&u32b(at));
        (blob, file)
    }

    /// A hand-built IFD pointing at a SHORT array is read back exactly.
    #[test]
    fn reads_a_short_array_through_a_file_offset() {
        let values: Vec<u16> = (0..100u16).collect();
        let (blob, file) = one_entry_ifd(TAG, &values, 64, Endian::Little);
        assert_eq!(
            parse(&blob, &file, TAG, 64..=8192, Endian::Little),
            Some(values)
        );
    }

    /// The EOS-1Ds case: the same IFD written the other way round. Reading it
    /// little-endian is what made the fix look like it had not run at all.
    #[test]
    fn reads_a_big_endian_ifd_too() {
        let values: Vec<u16> = (0..100u16).collect();
        let (blob, file) = one_entry_ifd(TAG, &values, 64, Endian::Big);
        assert_eq!(
            parse(&blob, &file, TAG, 64..=8192, Endian::Big),
            Some(values.clone())
        );
        assert!(
            parse(&blob, &file, TAG, 64..=8192, Endian::Little).is_none(),
            "read with the wrong byte order this must find nothing, not nonsense"
        );
    }

    /// The byte order comes from the file's own magic, and nothing else is a
    /// TIFF.
    #[test]
    fn byte_order_is_read_from_the_magic() {
        assert_eq!(Endian::of(b"II*\0"), Some(Endian::Little));
        assert_eq!(Endian::of(b"MM\0*"), Some(Endian::Big));
        assert_eq!(Endian::of(b"\xff\xd8ff"), None, "a JPEG is not a container");
        assert_eq!(Endian::of(b"M"), None, "too short to say");
    }

    /// An IFD without the tag, or with garbage in it, yields nothing rather
    /// than a made-up answer.
    #[test]
    fn declines_what_it_does_not_understand() {
        assert!(parse(&[0xff; 32], &[], TAG, 64..=8192, Endian::Little).is_none());
        assert!(parse(&[], &[], TAG, 64..=8192, Endian::Little).is_none());

        let mut blob = vec![0u8; 2 + 12];
        blob[0] = 1;
        blob[2..4].copy_from_slice(&0x0101u16.to_le_bytes());
        assert!(parse(&blob, &[], TAG, 64..=8192, Endian::Little).is_none());
    }

    /// The count is a claim about the tag's identity, not a formality: an array
    /// of the wrong length is declined rather than read.
    #[test]
    fn a_count_outside_the_range_is_declined() {
        let values: Vec<u16> = (0..10u16).collect();
        let (blob, file) = one_entry_ifd(TAG, &values, 64, Endian::Little);
        assert!(parse(&blob, &file, TAG, 64..=8192, Endian::Little).is_none());
        assert_eq!(
            parse(&blob, &file, TAG, 3..=4096, Endian::Little),
            Some(values)
        );
    }

    /// An offset running off the end of the file yields nothing.
    #[test]
    fn a_short_file_is_declined() {
        let values: Vec<u16> = (0..100u16).collect();
        let (blob, mut file) = one_entry_ifd(TAG, &values, 64, Endian::Little);
        file.truncate(100);
        assert!(parse(&blob, &file, TAG, 64..=8192, Endian::Little).is_none());
    }
}
