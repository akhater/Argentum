//! Reading the lens name out of a camera's private MakerNote block.
//!
//! WHY THIS EXISTS
//!
//! Auto lens detection failed on a Canon EOS 5D Mark II with an EF 135mm f/2 L,
//! while darktable identified it immediately. The information is in the file —
//! the string `EF135mm f/2L USM` is sitting in the CR2, verified by reading the
//! bytes — but nothing in our chain looks where it lives:
//!
//!   - `kamadak-exif` reads standard EXIF, and Canon does not write `LensModel`
//!     there on this body. It comes back `None`.
//!   - rawler's CR2 decoder does try (`get_lens_description`), but it looks for
//!     that same standard `LensModel` tag and, failing that, falls back to a
//!     numeric id from Canon's CameraSettings. For this lens the id maps to
//!     seven candidates and it gives up: *"Found multiple (7) lens definitions,
//!     unable to determine which lens to use"*.
//!
//! So `LensModel` arrives empty, `autodetect_lens` has nothing to match against,
//! and the lens has to be picked by hand — even though lensfun ships a profile
//! for it and the correction maths works perfectly once selected.
//!
//! darktable succeeds because exiv2 decodes MakerNotes. We are not taking on a
//! C++ dependency to read one string: a MakerNote is a standard TIFF IFD, so
//! this parses it directly. No new crates, and it stays in our own files.
//!
//! SCOPE
//!
//! Canon only, for now. Nikon, Sony and Fuji each use a different MakerNote
//! layout and would need their own reader — the entry point is shaped so one can
//! be added without disturbing this.

/// Canon MakerNote tag 0x0095 — the lens name as ASCII.
///
/// Present on most Canon bodies from roughly the 5D Mark II era onward. Older
/// ones only carry the numeric lens id, which is exactly the ambiguous value
/// rawler already fails on, so there is nothing gained by reading that too.
const CANON_LENS_MODEL: u16 = 0x0095;

/// TIFF field type 2: NUL-terminated ASCII.
const TYPE_ASCII: u16 = 2;

/// Sanity bound on the MakerNote IFD. Real ones hold a few dozen entries; a
/// wild count means we have mis-located the IFD and should stop rather than
/// walk megabytes of nonsense.
const MAX_ENTRIES: u16 = 512;

/// Longest plausible lens name. Guards against a corrupt length field turning
/// into a huge allocation.
const MAX_NAME_LEN: usize = 256;

/// The lens name a camera wrote into its own private metadata, if it is there.
///
/// Returns `None` for anything not understood — a different maker, an older
/// body, a file that does not parse. Detection then carries on exactly as it did
/// before, so a failure here can never be worse than the current behaviour.
pub fn read_lens_model(file_bytes: &[u8]) -> Option<String> {
    // Standard EXIF locates the MakerNote for us: it gives the offset and length
    // of the blob without having to walk the file structure by hand.
    let mut cursor = std::io::Cursor::new(file_bytes);
    let exif = exif::Reader::new().read_from_container(&mut cursor).ok()?;

    // Only Canon is understood so far. Reading another maker's block with
    // Canon's tag numbers would return confident nonsense.
    let make = exif
        .get_field(exif::Tag::Make, exif::In::PRIMARY)
        .map(|f| f.display_value().to_string())?;
    if !make.to_ascii_lowercase().contains("canon") {
        return None;
    }

    let field = exif.get_field(exif::Tag::MakerNote, exif::In::PRIMARY)?;
    let (blob, _) = match &field.value {
        exif::Value::Undefined(bytes, offset) => (bytes.as_slice(), *offset),
        _ => return None,
    };

    // Canon's MakerNote is a bare IFD — no header of its own — and the offsets
    // inside it are relative to the start of the TIFF file, not to the blob. So
    // the blob supplies the structure and the whole file supplies the values.
    parse_canon_ifd(blob, file_bytes)
}

/// Walk the IFD entries looking for the lens name.
///
/// Canon writes little-endian on every body carrying this tag. If that ever
/// stops being true the entry-count check catches it, since a byte-swapped count
/// is absurdly large.
fn parse_canon_ifd(blob: &[u8], file_bytes: &[u8]) -> Option<String> {
    let u16_at = |i: usize| -> Option<u16> {
        Some(u16::from_le_bytes([*blob.get(i)?, *blob.get(i + 1)?]))
    };
    let u32_at = |i: usize| -> Option<u32> {
        Some(u32::from_le_bytes([
            *blob.get(i)?,
            *blob.get(i + 1)?,
            *blob.get(i + 2)?,
            *blob.get(i + 3)?,
        ]))
    };

    let count = u16_at(0)?;
    if count == 0 || count > MAX_ENTRIES {
        return None;
    }

    for i in 0..count as usize {
        // A 2-byte entry count, then 12 bytes per entry: tag, type, count, value.
        let entry = 2 + i * 12;
        if u16_at(entry)? != CANON_LENS_MODEL {
            continue;
        }

        let kind = u16_at(entry + 2)?;
        let len = u32_at(entry + 4)? as usize;
        if kind != TYPE_ASCII || len == 0 || len > MAX_NAME_LEN {
            return None;
        }

        // Four bytes or fewer sit inline; anything longer is an offset from the
        // start of the file. A lens name is always the latter.
        let bytes = if len <= 4 {
            blob.get(entry + 8..entry + 8 + len)?
        } else {
            let at = u32_at(entry + 8)? as usize;
            file_bytes.get(at..at.checked_add(len)?)?
        };

        // A TIFF ASCII field holds NUL-*terminated* strings, and Canon stores
        // more than one of them here. Trimming only the trailing NULs glued the
        // next string onto the end of the name — `EF 85mm f/1.8 USMUSM`, which
        // naturally matched nothing in lensfun while looking almost right in the
        // metadata pane. Take the first string and stop.
        let text = String::from_utf8_lossy(bytes);
        let name = text.split('\0').next().unwrap_or("").trim().to_string();

        // Some bodies reserve the tag without filling it, leaving zeroes.
        if name.is_empty() {
            return None;
        }
        return Some(name);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A blob that is not an IFD at all must be declined, not misread.
    #[test]
    fn refuses_garbage() {
        assert!(parse_canon_ifd(&[0xff; 32], &[]).is_none());
        assert!(parse_canon_ifd(&[], &[]).is_none());
        assert!(parse_canon_ifd(&[0x02, 0x00], &[]).is_none(), "truncated entries");
    }

    /// A well-formed IFD without the lens tag returns nothing, rather than
    /// guessing at whatever tag happens to occupy that slot.
    #[test]
    fn returns_nothing_when_the_tag_is_absent() {
        let mut blob = vec![0u8; 2 + 12];
        blob[0] = 1;
        blob[2] = 0x01; // tag 0x0101
        blob[3] = 0x01;
        assert!(parse_canon_ifd(&blob, &[]).is_none());
    }

    /// A non-Canon file must be left alone — its MakerNote uses different tag
    /// numbers, so reading 0x0095 out of it would invent a lens.
    #[test]
    fn ignores_files_that_are_not_canon() {
        assert!(read_lens_model(b"not an image at all").is_none());
    }

    /// Only the first NUL-terminated string is taken. Canon packs several into
    /// this one field, and running them together produced `EF 85mm f/1.8 USMUSM`
    /// — which looked nearly right in the metadata pane and matched nothing.
    #[test]
    fn stops_at_the_first_nul() {
        let mut blob = vec![0u8; 2 + 12];
        blob[0] = 1;
        blob[2..4].copy_from_slice(&CANON_LENS_MODEL.to_le_bytes());
        blob[4..6].copy_from_slice(&TYPE_ASCII.to_le_bytes());
        blob[6..10].copy_from_slice(&20u32.to_le_bytes());
        blob[10..14].copy_from_slice(&40u32.to_le_bytes());

        let mut file = vec![0u8; 40];
        file.extend_from_slice(b"EF85mm f/1.8 USM\0USM");
        assert_eq!(
            parse_canon_ifd(&blob, &file).as_deref(),
            Some("EF85mm f/1.8 USM")
        );
    }

    /// Reads an inline value, exercising the short-string path.
    #[test]
    fn reads_a_short_inline_name() {
        let mut blob = vec![0u8; 2 + 12];
        blob[0] = 1;
        blob[2..4].copy_from_slice(&CANON_LENS_MODEL.to_le_bytes());
        blob[4..6].copy_from_slice(&TYPE_ASCII.to_le_bytes());
        blob[6..10].copy_from_slice(&3u32.to_le_bytes());
        blob[10..13].copy_from_slice(b"EF\0");
        assert_eq!(parse_canon_ifd(&blob, &[]).as_deref(), Some("EF"));
    }

    /// The end-to-end check, against the file the bug was found on.
    ///
    /// Ignored by default because it reads a specific photo off AK's disk:
    /// cargo test --lib mods::makernote_lens -- --ignored --nocapture
    #[test]
    #[ignore = "reads a specific file off AK's disk"]
    fn reads_the_lens_from_a_real_canon_file() {
        let path =
            r"C:\Users\you\ClaudeDesktop\photostack\g_A\2026-09-06_Canon EOS 5D Mark II_104-6382.CR2";
        let bytes = std::fs::read(path).expect("read sample");
        let name = read_lens_model(&bytes).expect("should find a lens name");
        println!("lens: {name}");
        assert!(name.contains("135"), "unexpected lens name: {name}");
    }
}


/// Fill in `LensModel` from the MakerNote when the standard EXIF pass did not
/// produce a usable one.
///
/// Called from `extract_metadata` immediately before it returns, which is the
/// only place that works: that function builds a map from standard EXIF and
/// returns the moment it is non-empty. Everything below that early return —
/// including their own lens handling and, at first, this fix — is dead code for
/// any file that carries ordinary EXIF, which is every file. The fix sat there
/// doing nothing until the early return was found.
///
/// An existing value always wins. A blank or whitespace-only one does not: a
/// present-but-empty field quietly beating a good name is the same class of
/// failure as the early return, and would be just as invisible.
pub fn fill_lens_model(map: &mut std::collections::HashMap<String, String>, file_bytes: &[u8]) {
    let already_good = map
        .get("LensModel")
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false);

    if already_good {
        return;
    }

    if let Some(raw) = read_lens_model(file_bytes) {
        let name = normalise_lens_name(&raw);
        log::info!("[lens] MakerNote supplied LensModel = {raw:?} -> {name:?}");
        map.insert("LensModel".to_string(), name);
    }
}

#[cfg(test)]
mod folder_scan {
    use super::*;

    /// Scan a real folder and report the lens for every file.
    ///
    /// Used to tell "this file has no lens tag" apart from "this file was cached
    /// before the fix" — the two look identical in the UI.
    ///
    /// cargo test --lib mods::makernote_lens::folder_scan -- --ignored --nocapture
    #[test]
    #[ignore = "scans a folder on AK's disk"]
    fn every_file_in_a_folder() {
        let dir = std::path::Path::new(
            r"C:\Users\you\OneDrive\Pictures\_Original to Review\2026\2026-09-06",
        );
        let Ok(entries) = std::fs::read_dir(dir) else {
            println!("folder not readable: {}", dir.display());
            return;
        };

        let mut missing = 0;
        let mut found = 0;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase())
                != Some("cr2".to_string())
            {
                continue;
            }
            let Ok(bytes) = std::fs::read(&path) else { continue };
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            match read_lens_model(&bytes) {
                Some(lens) => {
                    found += 1;
                    println!("OK      {name}  ->  {lens}");
                }
                None => {
                    missing += 1;
                    println!("MISSING {name}");
                }
            }
        }
        println!("\nfound {found}, missing {missing}");
    }
}

/// Nudge a camera's lens string toward lensfun's spelling.
///
/// Canon writes the mount and focal length joined — `EF135mm f/2L USM` — while
/// lensfun separates them: `Canon EF 135mm f/2L`. Their matcher is fuzzy, so it
/// tolerates a lot, but a subsequence match on a joined string can score an
/// unrelated lens higher than the right one. An EF 85mm was being identified as
/// a "50mm f/1.5 (1.6x crop)".
///
/// Only the separator is touched. Nothing is added, removed or reordered — a
/// guess that rewrites the name into something the camera did not say would be
/// worse than the ambiguity it replaces.
fn normalise_lens_name(name: &str) -> String {
    // Longest first, so EF-S is not matched as EF.
    const MOUNTS: [&str; 6] = ["EF-S", "EF-M", "TS-E", "MP-E", "RF", "EF"];

    let trimmed = name.trim();
    for mount in MOUNTS {
        if let Some(rest) = trimmed.strip_prefix(mount)
            && rest.starts_with(|c: char| c.is_ascii_digit())
        {
            return format!("{mount} {rest}");
        }
    }
    trimmed.to_string()
}

#[cfg(test)]
mod naming {
    use super::*;

    #[test]
    fn separates_the_mount_from_the_focal_length() {
        assert_eq!(normalise_lens_name("EF135mm f/2L USM"), "EF 135mm f/2L USM");
        assert_eq!(normalise_lens_name("EF85mm f/1.8 USM"), "EF 85mm f/1.8 USM");
        assert_eq!(normalise_lens_name("EF-S18-55mm f/3.5-5.6"), "EF-S 18-55mm f/3.5-5.6");
        assert_eq!(normalise_lens_name("RF50mm F1.2 L USM"), "RF 50mm F1.2 L USM");
    }

    /// Already-spaced and non-Canon-style names must pass through untouched.
    #[test]
    fn leaves_anything_else_alone() {
        assert_eq!(normalise_lens_name("EF 135mm f/2L USM"), "EF 135mm f/2L USM");
        assert_eq!(normalise_lens_name("Sigma 35mm f/1.4 DG HSM"), "Sigma 35mm f/1.4 DG HSM");
        assert_eq!(normalise_lens_name("EFxyz"), "EFxyz");
        assert_eq!(normalise_lens_name(""), "");
    }
}
