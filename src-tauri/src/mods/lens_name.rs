//! The lens a photo was taken with, whatever kind of file it is. Ours.
//!
//! WHY THIS EXISTS
//!
//! 26.37.6 made lens detection work by reading Canon's MakerNote
//! (`makernote_lens`). It was tested on a folder of CR2s and on nothing else,
//! and it reached the file through `kamadak-exif`, which cannot open a CR3 at
//! all: a CR3 is ISO-BMFF, not TIFF. So on every CR3 the fix did nothing, and
//! the lens name came from one place only — rawler's own table of lens ids,
//! which names a lens only when it knows the id. An EF 100-400mm f/4.5-5.6L IS
//! USM on an R body reports id `183:0`, which it does not know:
//!
//! ```text
//! No lens definition found in database, search parameters: ... ID: '183:0',
//! Keyname: 'EF100-400mm f/4.5-5.6L IS USM'
//! ```
//!
//! The camera had written the name in plain text — rawler printed it — and it
//! was dropped because the id lookup failed. Empty `LensModel`, "Lens profile
//! not found", on a lens lensfun has two calibrations for.
//!
//! Fixing CR3 alone would have been the same mistake twice. Every camera that
//! names its lens writes it to the same standard tag, EXIF `LensModel`; formats
//! differ only in *where the EXIF block lives*. So that is the one thing this
//! varies on:
//!
//!   1. A container kamadak opens: TIFF and the RAWs built on it (CR2, NEF,
//!      ARW, DNG, PEF ...), JPEG, HEIF, PNG, WebP.
//!   2. A TIFF with a private magic number — Olympus ORF, Panasonic RW2. An
//!      ordinary TIFF in every other respect; kamadak refuses it on the magic.
//!   3. Anything else — CR3, Fuji RAF — carries its EXIF as a complete TIFF
//!      block inside the container. Found by its header, wherever it sits.
//!
//! Then, for a body that never wrote the standard tag, its MakerNote
//! (`makernote_lens`, Canon only so far).
//!
//! It only ever fills a blank `LensModel`. A name that already arrived — from
//! standard EXIF or from rawler's table — is left exactly as it was, so nothing
//! that worked before can change.

use std::collections::HashMap;
use std::io::Cursor;

/// EXIF `LensModel`.
const LENS_MODEL: u16 = 0xa434;

/// How far into a file to look for an embedded EXIF block. CR3 keeps its
/// metadata boxes in `moov`, at the front; a RAF's EXIF is in the preview JPEG
/// straight after its header. Both sit within the first few hundred KB.
const SCAN_LIMIT: usize = 8 << 20;

/// Most TIFF headers worth parsing. A CR3 has four (CMT1 to CMT4); past a
/// handful, a hit is image data that happens to contain the four bytes.
const MAX_BLOCKS: usize = 16;

/// Most bytes handed to the parser per embedded block. An EXIF block is tens of
/// KB; this bounds the copy without cutting a real one short.
const BLOCK_LIMIT: usize = 1 << 20;

/// Their fresh EXIF read, with the lens filled in if it came back without one.
///
/// The single call in their `exif_processing.rs`, in `read_exif_data`, around
/// `read_exif_data_from_bytes`. That is the one point every format passes
/// through on a fresh read — RAW by either of `extract_metadata`'s two exits,
/// JPEG and TIFF by the other branch — and it comes before the result is
/// cached, so the lens is saved with the rest and the next open is free.
///
/// The first fix sat inside `extract_metadata`, before its early return. That
/// covered only RAWs kamadak opens; JPEG, TIFF, and every RAW that reaches
/// rawler's path went past it.
///
/// EXIF cached before this existed never comes back here: that cache is keyed
/// on the photo, not the app. `recover_lens_name` is for those.
pub fn with_lens(mut map: HashMap<String, String>, file_bytes: &[u8]) -> HashMap<String, String> {
    fill_lens_model(&mut map, file_bytes);
    map
}

/// Fill in `LensModel` when nothing before this produced a usable one.
///
/// A blank or whitespace-only value counts as missing: a present-but-empty
/// field quietly beating a good name is the failure this keeps fixing.
///
/// An empty map is left empty. Nothing at all was read, and a lens on its own
/// is not metadata.
pub fn fill_lens_model(map: &mut HashMap<String, String>, file_bytes: &[u8]) {
    if map.is_empty() || has_lens(map) {
        return;
    }

    if let Some(name) = read_lens_name(file_bytes) {
        log::info!("[lens] read LensModel {name:?} from the file");
        map.insert("LensModel".to_string(), name);
    }
}

/// The lens name in the photo at `path`, read from the file and written into
/// the EXIF cached for it.
///
/// For EXIF cached before `with_lens` existed. A photo opened while CR3 lenses
/// went unread has an empty `LensModel` saved for it, and nothing about the app
/// changing invalidates that cache.
///
/// Handing the name to the frontend alone is not enough. Their backend detects
/// the lens again from the cached EXIF every time an edit is saved
/// (`resolve_lens_params_in_adjustments`) and, finding none there, deletes the
/// lens from the edit. So the name goes into the cache, through their own
/// `update_exif_fields` - the path the metadata panel's edits take, which
/// writes it into the photo's sidecar, creates one if needed, and leaves every
/// other field as it was, including any the user edited.
pub async fn recover_lens_name(path: String) -> Result<Option<String>, String> {
    let (source, _) = crate::file_management::parse_virtual_path(&path);
    let name = {
        let bytes = crate::file_management::read_file_mapped(&source).map_err(|e| e.to_string())?;
        read_lens_name(&bytes)
    };

    if let Some(name) = &name {
        log::info!(
            "[lens] recovered LensModel {name:?} for {}",
            source.display()
        );
        crate::file_management::update_exif_fields(
            vec![source.to_string_lossy().to_string()],
            HashMap::from([("LensModel".to_string(), name.clone())]),
        )
        .await?;
    }
    Ok(name)
}

fn has_lens(map: &HashMap<String, String>) -> bool {
    map.get("LensModel")
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
}

/// The lens name in a file of any format, if the camera wrote one.
pub fn read_lens_name(file_bytes: &[u8]) -> Option<String> {
    let mut makernote = None;
    for exif in exif_blocks(file_bytes) {
        if let Some(name) = standard_lens_model(&exif) {
            return Some(crate::mods::makernote_lens::normalise_lens_name(&name));
        }
        if makernote.is_none() {
            makernote = crate::mods::makernote_lens::from_exif(&exif);
        }
    }
    makernote.map(|name| crate::mods::makernote_lens::normalise_lens_name(&name))
}

/// Every EXIF block in the file, nearest to hand first, parsed only as needed.
fn exif_blocks(file_bytes: &[u8]) -> Box<dyn Iterator<Item = exif::Exif> + '_> {
    if let Some(exif) = parse(reader().read_from_container(&mut Cursor::new(file_bytes))) {
        return Box::new(std::iter::once(exif));
    }

    Box::new(
        std::iter::once_with(move || private_magic(file_bytes))
            .flatten()
            .chain(embedded(file_bytes)),
    )
}

/// A TIFF that calls itself something else. Parsed as the TIFF it is.
fn private_magic(file_bytes: &[u8]) -> Option<exif::Exif> {
    let header = file_bytes.get(..4)?;
    let magic: [u8; 2] = match &header[..2] {
        b"II" => [42, 0],
        b"MM" => [0, 42],
        _ => return None,
    };
    if header[2..4] == magic {
        // A real TIFF, which kamadak has already turned down.
        return None;
    }

    let mut patched = file_bytes.to_vec();
    patched[2..4].copy_from_slice(&magic);
    parse(reader().read_raw(patched))
}

/// TIFF blocks embedded in a container that is not itself a TIFF.
fn embedded(file_bytes: &[u8]) -> impl Iterator<Item = exif::Exif> + '_ {
    let end = file_bytes.len().min(SCAN_LIMIT);
    (0..end.saturating_sub(8))
        .filter(move |&at| matches!(&file_bytes[at..at + 4], b"II*\0" | b"MM\0*"))
        .take(MAX_BLOCKS)
        .filter_map(move |at| {
            let block = &file_bytes[at..file_bytes.len().min(at + BLOCK_LIMIT)];
            parse(reader().read_raw(block.to_vec()))
        })
}

/// Lenient: a block whose last field runs off the end, or that we cut short at
/// `BLOCK_LIMIT`, still gives up the fields it does hold.
fn reader() -> exif::Reader {
    let mut reader = exif::Reader::new();
    reader.continue_on_error(true);
    reader
}

fn parse(result: Result<exif::Exif, exif::Error>) -> Option<exif::Exif> {
    let exif = match result {
        Ok(exif) => exif,
        Err(exif::Error::PartialResult(partial)) => partial.into_inner().0,
        Err(_) => return None,
    };
    let any = exif.fields().next().is_some();
    any.then_some(exif)
}

/// `LensModel`, matched on the tag number alone. In a CR3 the EXIF IFD *is*
/// the block's first IFD, so kamadak files its tags under the TIFF context
/// rather than EXIF, and `Tag::LensModel` would not match it.
fn standard_lens_model(exif: &exif::Exif) -> Option<String> {
    exif.fields()
        .filter(|field| field.tag.number() == LENS_MODEL)
        .find_map(|field| match &field.value {
            exif::Value::Ascii(parts) => parts
                .iter()
                .map(|part| String::from_utf8_lossy(part).trim().to_string())
                // Some bodies write the tag with no lens fitted: blank, or "----".
                .find(|name| name.chars().any(|c| c.is_ascii_alphanumeric())),
            _ => None,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal little-endian TIFF: one IFD holding one ASCII `LensModel`.
    fn tiff_with_lens(name: &str) -> Vec<u8> {
        let mut text = name.as_bytes().to_vec();
        text.push(0);
        let mut tiff = b"II*\0".to_vec();
        tiff.extend_from_slice(&8u32.to_le_bytes());
        tiff.extend_from_slice(&1u16.to_le_bytes());
        tiff.extend_from_slice(&LENS_MODEL.to_le_bytes());
        tiff.extend_from_slice(&2u16.to_le_bytes());
        tiff.extend_from_slice(&(text.len() as u32).to_le_bytes());
        tiff.extend_from_slice(&26u32.to_le_bytes());
        tiff.extend_from_slice(&0u32.to_le_bytes());
        tiff.extend_from_slice(&text);
        tiff
    }

    /// The CR3 case: EXIF inside a container kamadak does not know, with
    /// `LensModel` in the block's first IFD.
    #[test]
    fn finds_the_lens_in_an_embedded_block() {
        let mut file = b"\0\0\0\x18ftypcrx \0\0\0\x01crx isom".to_vec();
        file.extend_from_slice(&[0u8; 100]);
        file.extend_from_slice(&tiff_with_lens("EF100-400mm f/4.5-5.6L IS USM"));
        file.extend_from_slice(&[0u8; 100]);
        assert_eq!(
            read_lens_name(&file).as_deref(),
            Some("EF 100-400mm f/4.5-5.6L IS USM")
        );
    }

    /// The ORF/RW2 case: a TIFF under another magic number.
    #[test]
    fn reads_a_tiff_with_a_private_magic() {
        let mut file = tiff_with_lens("M.12-40mm F2.8");
        file[2..4].copy_from_slice(b"RO");
        assert_eq!(read_lens_name(&file).as_deref(), Some("M.12-40mm F2.8"));
    }

    #[test]
    fn a_placeholder_is_not_a_lens() {
        let mut file = b"FUJIFILMCCD-RAW ".to_vec();
        file.extend_from_slice(&tiff_with_lens("----"));
        assert!(read_lens_name(&file).is_none());
    }

    #[test]
    fn leaves_an_existing_name_and_an_empty_map_alone() {
        let file = tiff_with_lens("EF85mm f/1.8 USM");

        let mut empty = HashMap::new();
        fill_lens_model(&mut empty, &file);
        assert!(
            empty.is_empty(),
            "a lens alone would end extract_metadata early"
        );

        let mut named = HashMap::from([("LensModel".to_string(), "Theirs".to_string())]);
        fill_lens_model(&mut named, &file);
        assert_eq!(named["LensModel"], "Theirs");

        let mut blank = HashMap::from([
            ("Make".to_string(), "Canon".to_string()),
            ("LensModel".to_string(), "  ".to_string()),
        ]);
        fill_lens_model(&mut blank, &file);
        assert_eq!(blank["LensModel"], "EF 85mm f/1.8 USM");
    }

    #[test]
    fn declines_garbage() {
        assert!(read_lens_name(b"not an image at all").is_none());
        assert!(read_lens_name(&[]).is_none());
    }

    /// Every photo in a folder, through the path the Auto button takes: the
    /// app's own EXIF read, then their lensfun matcher against the bundled
    /// database. "Lens profile not found" is the MISSING line.
    ///
    /// ```text
    /// AG_LENS_DIR=... cargo test --lib mods::lens_name -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "reads the folder named by AG_LENS_DIR"]
    fn every_file_in_a_folder() {
        use crate::lens_correction::{LensDatabase, find_best_lens_match};

        let dir = std::env::var("AG_LENS_DIR").expect("set AG_LENS_DIR to a folder of photos");

        let mut db = LensDatabase {
            cameras: Vec::new(),
            lenses: Vec::new(),
        };
        let lensfun = concat!(env!("CARGO_MANIFEST_DIR"), "/lensfun_db");
        for entry in std::fs::read_dir(lensfun).expect("lensfun_db").flatten() {
            let xml = std::fs::read_to_string(entry.path()).unwrap_or_default();
            if let Ok(mut part) = quick_xml::de::from_str::<LensDatabase>(&xml) {
                db.cameras.append(&mut part.cameras);
                db.lenses.append(&mut part.lenses);
            }
        }

        let (mut found, mut missing) = (0, 0);
        for entry in std::fs::read_dir(&dir).expect("read folder").flatten() {
            let path = entry.path();
            let photo = path.to_string_lossy().to_string();
            if !crate::formats::is_supported_image_file(&path)
                || entry
                    .metadata()
                    .map(|m| m.len() < 64 * 1024)
                    .unwrap_or(true)
            {
                continue;
            }
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            let name = path.file_name().unwrap().to_string_lossy().to_string();

            let started = std::time::Instant::now();
            // As their read_exif_data does, minus its cache.
            let exif = with_lens(
                crate::exif_processing::read_exif_data_from_bytes(&photo, &bytes),
                &bytes,
            );
            let read = started.elapsed();
            let get = |key: &str| exif.get(key).cloned().unwrap_or_default();
            let (make, lens, body) = (get("Make"), get("LensModel"), get("Model"));

            match find_best_lens_match(&db, &make, &lens, &body).filter(|_| !lens.is_empty()) {
                Some((maker, model)) => {
                    found += 1;
                    println!("OK      {name}  {body} | {lens}  ->  {maker} {model}  ({read:?})");
                }
                None => {
                    missing += 1;
                    println!("MISSING {name}  {body} | {lens:?}  ({read:?})");
                }
            }
        }
        println!("\nfound {found}, missing {missing}");
    }
}
