//! Where the camera focused: the autofocus points a photo's maker note
//! records, drawn over the photo in the photo view (Focus points, Shift+F).
//! Ours, from Shoebox.
//!
//! The reader is Shoebox's MIT crate `focus-points` (Cargo.toml): Canon,
//! Nikon, Sony, Fujifilm, Olympus / OM System, Panasonic / Leica and Pentax /
//! Ricoh maker notes, and any maker's EXIF SubjectArea (phones). This module
//! finds a file's maker note with rawler and gives the crate its directories.
//! Both halves are Shoebox's, at 580c5a6d, on the same rawler revision:
//! the adapter from `src-tauri/src/focus.rs` (`Tags`), the finding from
//! `src-tauri/src/imaging.rs` (`with_notes` and what it calls).
//!
//! Points are fractions of the picture as it stands upright by its EXIF
//! orientation, the frame the editor opens it in; the user's own turns, flips
//! and crop are the interface's to apply (src/argentum/focusPlacement.ts).

use std::io::{Cursor, Read, Seek};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use focus_points::{Directory, Value};
use rawler::bits::Endian;
use rawler::formats::tiff::{GenericTiffReader, IFD, Value as TiffValue, reader::TiffReader};
use rawler::rawsource::RawSource;
use serde::Serialize;

use super::ag_settings::Setting;

/// Whether the points are drawn: one switch for every photo, kept across restarts.
const SHOWN: Setting<bool> = Setting::new("focusPointsShown");

/// Whether the points were left drawn; off until switched on.
pub fn shown(library: &Path) -> bool {
    SHOWN.load(library).unwrap_or(false)
}

pub fn save_shown(library: &Path, on: bool) -> Result<(), String> {
    SHOWN.save(library, &on)
}

/// Where the camera focused, as the interface receives it: the crate's
/// `Focus`, field for field. `From` takes each apart whole, so a field the
/// crate adds stops this compiling rather than going missing.
#[derive(Serialize, Debug, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Focus {
    /// How the camera was set to focus, in the crate's `MODES` words
    /// ("Manual focus", "Single point"...).
    pub mode: Option<&'static str>,
    pub points: Vec<FocusPoint>,
}

/// One point: its top-left corner and size, fractions of the upright picture.
#[derive(Serialize, Debug, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct FocusPoint {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub state: FocusState,
}

/// What a point was in this photo.
#[derive(Serialize, Debug, PartialEq, Clone, Copy)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum FocusState {
    /// One of the camera's points, not used for this photo.
    Other,
    /// Chosen, the camera not saying focus was found there.
    Chosen,
    /// Focus was found there.
    InFocus,
    /// The zone the camera was given to look in: an outline, not a point.
    Area,
}

impl From<focus_points::Focus> for Focus {
    fn from(f: focus_points::Focus) -> Self {
        let focus_points::Focus { mode, points } = f;
        let points = points
            .into_iter()
            .map(|p| {
                let focus_points::Point { x, y, w, h, state } = p;
                let state = match state {
                    focus_points::State::Other => FocusState::Other,
                    focus_points::State::Chosen => FocusState::Chosen,
                    focus_points::State::InFocus => FocusState::InFocus,
                    focus_points::State::Area => FocusState::Area,
                };
                FocusPoint { x, y, w, h, state }
            })
            .collect();
        Focus { mode, points }
    }
}

/// The `ag` command `focus_points`: where the photo at `path` was focused,
/// from its metadata only; `None` when its file records nothing.
pub fn of_photo(path: &str) -> Result<Option<Focus>, String> {
    let (source, _) = crate::file_management::parse_virtual_path(path);
    let bytes = crate::file_management::read_file_mapped(&source).map_err(|e| e.to_string())?;
    let raw = crate::formats::is_raw_file(&source);
    // A maker note is someone else's format: one that is not what it says gives nothing, not a crash.
    Ok(catch_unwind(AssertUnwindSafe(|| of_file(&bytes[..], raw)))
        .ok()
        .flatten()
        .map(Focus::from))
}

/// A RAW's points, read where each format keeps its metadata; another
/// file's (a JPEG's, a HEIC's) from its EXIF, unless it was cropped since.
fn of_file(bytes: &[u8], raw: bool) -> Option<focus_points::Focus> {
    if raw {
        return with_notes(&RawSource::new_from_slice(bytes), |n| note_focus(n, None));
    }
    let ex = exif::Reader::new()
        .read_from_container(&mut Cursor::new(bytes))
        .ok()?;
    let size = |tag| {
        ex.get_field(tag, exif::In::PRIMARY)
            .and_then(|f| f.value.get_uint(0))
    };
    let picture = size(exif::Tag::PixelXDimension).zip(size(exif::Tag::PixelYDimension));
    exif_notes(&ex, |n| note_focus(n, picture))
}

// ---- The adapter: Shoebox src-tauri/src/focus.rs at 580c5a6d --------------

/// A directory of rawler's, as `focus-points` reads tags.
#[derive(Clone, Copy)]
struct Tags<'a>(&'a IFD);

impl Directory for Tags<'_> {
    fn value(&self, tag: u16) -> Option<Value<'_>> {
        Some(match &self.0.get_entry(tag)?.value {
            TiffValue::Undefined(b) | TiffValue::Byte(b) => Value::Bytes(b),
            TiffValue::Short(v) => Value::Short(v),
            TiffValue::SShort(v) => Value::SShort(v),
            TiffValue::Long(v) => Value::Long(v),
            TiffValue::SLong(v) => Value::SLong(v),
            TiffValue::Rational(v) => Value::Rational(v.iter().map(|r| (r.n, r.d)).collect()),
            TiffValue::SRational(v) => Value::SRational(v.iter().map(|r| (r.n, r.d)).collect()),
            _ => return None,
        })
    }

    fn sub(&self, tag: u16) -> Option<Self> {
        self.0.get_sub_ifd(tag).map(Tags)
    }

    fn big_endian(&self) -> bool {
        matches!(self.0.endian, Endian::Big)
    }
}

// ---- Finding the maker note: Shoebox src-tauri/src/imaging.rs at 580c5a6d --

const MAKE: u16 = 0x010F;
const MODEL: u16 = 0x0110;
const ORIENTATION: u16 = 0x0112;
const EXIF_IFD: u16 = 0x8769;
const MAKER_NOTE: u16 = 0x927C;
/// Where a DNG keeps the camera's original maker note.
const DNG_PRIVATE_DATA: u16 = 0xC634;

/// A file's metadata directories, as its maker's note is read: the maker
/// (lower case) and model, its EXIF and main directories, the note itself,
/// and the EXIF orientation.
struct Notes<'a> {
    make: &'a str,
    model: &'a str,
    ifds: &'a [&'a IFD],
    note: Option<&'a IFD>,
    orientation: u16,
}

/// Where the camera focused, as the picture stands upright. `picture`: its
/// size as the file stores it, when the file may have been edited (a JPEG):
/// points of a picture cropped since are left out.
fn note_focus(n: &Notes, picture: Option<(u32, u32)>) -> Option<focus_points::Focus> {
    let ifds: Vec<Tags> = n.ifds.iter().map(|i| Tags(i)).collect();
    let src = focus_points::Source {
        make: n.make,
        model: n.model,
        ifds: &ifds,
        note: n.note.map(Tags),
    };
    focus_points::read(&src, picture).map(|f| f.upright(n.orientation))
}

/// `f` of a RAW file's metadata, read where each format keeps it: the
/// TIFF-based RAWs (CR2, NEF, ARW, ORF, PEF, RW2, DNG...), a CR3's boxes, the
/// JPEG inside a Fuji RAF.
fn with_notes<T: Default>(src: &RawSource, f: impl FnOnce(&Notes) -> T) -> T {
    let buf = src.buf();
    let Some(head) = buf.get(..16) else {
        return T::default();
    };
    if &head[4..8] == b"ftyp" {
        let Ok(bmff) = rawler::formats::bmff::Bmff::new(src.reader()) else {
            return T::default();
        };
        let Some(cr3) = bmff.filebox.moov.cr3desc.as_ref() else {
            return T::default();
        };
        let ifds = [cr3.cmt2.tiff.root_ifd(), cr3.cmt1.tiff.root_ifd()];
        let orientation = short(ifds[1], ORIENTATION).unwrap_or(0);
        let model = ascii(ifds[1], MODEL).unwrap_or_default();
        let note = Some(cr3.cmt3.tiff.root_ifd());
        return f(&Notes {
            make: "canon",
            model: &model,
            ifds: &ifds,
            note,
            orientation,
        });
    }
    if head.starts_with(b"FUJIFILM") {
        // The camera's JPEG: offset and length at 84, big-endian. Its EXIF is
        // at its start, so a file cut short still has it.
        let at = |i: usize| {
            buf.get(i..i + 4)
                .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize)
        };
        let jpeg = at(84)
            .zip(at(88))
            .and_then(|(off, len)| buf.get(off..off.saturating_add(len).min(buf.len())));
        return match jpeg.and_then(|j| {
            exif::Reader::new()
                .read_from_container(&mut Cursor::new(j))
                .ok()
        }) {
            Some(ex) => exif_notes(&ex, f),
            None => T::default(),
        };
    }
    if head.starts_with(b"II") || head.starts_with(b"MM") {
        let mut r = src.reader();
        if let Ok(tiff) = GenericTiffReader::new(&mut r, 0, 0, Some(0), &[]) {
            return tiff_notes(&tiff, &mut r, f);
        }
    }
    T::default()
}

/// `f` of a parsed EXIF block's directories (a JPEG's, a HEIC's...).
fn exif_notes<T: Default>(ex: &exif::Exif, f: impl FnOnce(&Notes) -> T) -> T {
    let buf = ex.buf();
    match GenericTiffReader::new_with_buffer(buf, 0, 0, Some(0)) {
        Ok(tiff) => tiff_notes(&tiff, &mut Cursor::new(buf), f),
        Err(_) => T::default(),
    }
}

fn tiff_notes<R: Read + Seek, T>(
    tiff: &GenericTiffReader,
    r: &mut R,
    f: impl FnOnce(&Notes) -> T,
) -> T {
    let root = tiff.root_ifd();
    let exif = root.get_sub_ifd(EXIF_IFD);
    let make = ascii(root, MAKE).unwrap_or_default().to_ascii_lowercase();
    let model = ascii(root, MODEL).unwrap_or_default();
    let note = exif
        .and_then(|e| maker_note(e, r))
        .or_else(|| dng_maker_note(root, r));
    let ifds: Vec<&IFD> = exif.into_iter().chain(std::iter::once(root)).collect();
    let orientation = short(root, ORIENTATION).unwrap_or(0);
    f(&Notes {
        make: &make,
        model: &model,
        ifds: &ifds,
        note: note.as_ref(),
        orientation,
    })
}

/// A maker note's directory, wherever its maker puts it: Nikon's has its own
/// TIFF header (offsets from there), Pentax's a name and a byte order, Canon's
/// is a plain directory. `corr` moves offsets written for another file (a
/// maker note copied into a DNG).
fn note_ifd<R: Read + Seek>(
    r: &mut R,
    at: u32,
    data: &[u8],
    endian: Endian,
    corr: i32,
) -> Option<IFD> {
    let order = |b: &[u8]| match b {
        b"II" => Some(Endian::Little),
        b"MM" => Some(Endian::Big),
        _ => None,
    };
    // Older Nikon compacts: a name, then the directory; offsets from the file's TIFF header.
    if data.starts_with(b"Nikon\0\x01") {
        return IFD::new(r, at + 8, 0, corr, endian, &[]).ok();
    }
    if data.starts_with(b"Nikon\0") {
        return IFD::new(r, 8, at + 10, 0, order(data.get(10..12)?)?, &[]).ok();
    }
    // Sony (and Hasselblad's Sony-made bodies), Panasonic and Leica's
    // Panasonic-made ones: a name, then the directory; offsets from the
    // file's TIFF header.
    let named = [
        &b"SONY DSC "[..],
        b"SONY CAM ",
        b"SONY MOBILE",
        b"VHAB     ",
        b"Panasonic\0",
    ];
    if named.iter().any(|h| data.starts_with(h)) {
        return IFD::new(r, at + 12, 0, corr, endian, &[]).ok();
    }
    if data.starts_with(b"LEICA\0\0\0") {
        return IFD::new(r, at + 8, 0, corr, endian, &[]).ok();
    }
    // Fujifilm: its name, then where its directory starts; offsets from the
    // note's own start, always little-endian.
    if data.starts_with(b"FUJIFILM") {
        let start = u32::from_le_bytes(data.get(8..12)?.try_into().ok()?);
        return IFD::new(r, at + start, 0, at as i32, Endian::Little, &[]).ok();
    }
    // Olympus (since about 2004) and OM System: a name and a byte order, then
    // the directory, its camera settings a directory of their own; offsets
    // from the note's own start.
    let sub = [focus_points::OLYMPUS_CAMERA_SETTINGS];
    if data.starts_with(b"OLYMPUS\0") {
        return IFD::new(r, at + 12, 0, at as i32, order(data.get(8..10)?)?, &sub).ok();
    }
    if data.starts_with(b"OM SYSTEM\0") {
        return IFD::new(r, at + 16, 0, at as i32, order(data.get(12..14)?)?, &sub).ok();
    }
    if data.starts_with(b"AOC\0") {
        return IFD::new(
            r,
            at + 6,
            0,
            corr,
            order(data.get(4..6)?).unwrap_or(endian),
            &[],
        )
        .ok();
    }
    if data.starts_with(b"PENTAX ") {
        // Offsets from the note's own start.
        return IFD::new(r, at + 10, 0, at as i32, order(data.get(8..10)?)?, &[]).ok();
    }
    IFD::new(r, at, 0, corr, endian, &[]).ok()
}

fn maker_note<R: Read + Seek>(exif: &IFD, r: &mut R) -> Option<IFD> {
    let entry = exif.get_entry(MAKER_NOTE)?;
    let TiffValue::Undefined(data) = &entry.value else {
        return None;
    };
    note_ifd(r, entry.offset()? as u32, data, exif.endian, 0)
}

/// The camera's maker note a DNG kept: "Adobe\0", "MakN", its length, its
/// byte order, its offset in the original file, then the note.
fn dng_maker_note<R: Read + Seek>(root: &IFD, r: &mut R) -> Option<IFD> {
    let entry = root.get_entry(DNG_PRIVATE_DATA)?;
    let data = match &entry.value {
        TiffValue::Byte(d) | TiffValue::Undefined(d) => d,
        _ => return None,
    };
    // A Pentax's own DNG keeps its note there as it is.
    if data.starts_with(b"PENTAX ") {
        return note_ifd(r, entry.offset()? as u32, data, root.endian, 0);
    }
    if !data.starts_with(b"Adobe\0MakN") {
        return None;
    }
    let endian = match data.get(14..16)? {
        b"II" => Endian::Little,
        b"MM" => Endian::Big,
        _ => return None,
    };
    let original = u32::from_be_bytes(data.get(16..20)?.try_into().ok()?);
    let at = entry.offset()? as u32 + 20;
    note_ifd(r, at, data.get(20..)?, endian, at as i32 - original as i32)
}

fn short(ifd: &IFD, tag: u16) -> Option<u16> {
    match &ifd.get_entry(tag)?.value {
        TiffValue::Short(v) => v.first().copied(),
        _ => None,
    }
}

fn ascii(ifd: &IFD, tag: u16) -> Option<String> {
    match &ifd.get_entry(tag)?.value {
        TiffValue::Ascii(a) => a
            .strings()
            .first()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    type Entry = (u16, u16, u32, Vec<u8>);

    /// A little-endian directory as it sits at `at` in a file, its values
    /// right after it, offsets from the file's start: (tag, type, count,
    /// value bytes). From Shoebox's focus.rs tests.
    fn ifd_at(at: usize, entries: &[Entry]) -> Vec<u8> {
        let mut entries = entries.to_vec();
        entries.sort_by_key(|e| e.0);
        let data_at = at + 2 + entries.len() * 12 + 4;
        let mut out = (entries.len() as u16).to_le_bytes().to_vec();
        let mut data: Vec<u8> = Vec::new();
        for (tag, typ, count, bytes) in &entries {
            out.extend(tag.to_le_bytes());
            out.extend(typ.to_le_bytes());
            out.extend(count.to_le_bytes());
            if bytes.len() <= 4 {
                out.extend(bytes.iter().copied().chain(std::iter::repeat(0)).take(4));
            } else {
                out.extend(((data_at + data.len()) as u32).to_le_bytes());
                data.extend(bytes);
            }
        }
        out.extend(0u32.to_le_bytes());
        out.extend(data);
        out
    }

    /// A little-endian TIFF header, then `root` as its first directory.
    fn tiff(root: &[Entry]) -> Vec<u8> {
        let mut out = b"II*\0".to_vec();
        out.extend(8u32.to_le_bytes());
        out.extend(ifd_at(8, root));
        out
    }

    fn dir(entries: &[Entry]) -> IFD {
        GenericTiffReader::new_with_buffer(tiff(entries), 0, 0, Some(0))
            .unwrap()
            .root_ifd()
            .clone()
    }

    fn short_tag(tag: u16, v: &[u16]) -> Entry {
        (
            tag,
            3,
            v.len() as u32,
            v.iter().flat_map(|x| x.to_le_bytes()).collect(),
        )
    }

    /// A 5D Mark II's AFInfo2 as its CR2 keeps it: nine points in a diamond,
    /// the right one in focus and chosen. From Shoebox's focus.rs tests.
    fn five_d_mark_ii() -> Vec<u16> {
        let mut v = vec![96, 2, 9, 9, 5616, 3744, 5616, 3744];
        v.extend([84, 84, 101, 84, 84, 84, 101, 84, 123]);
        v.extend([101, 101, 74, 101, 101, 101, 74, 101, 131]);
        v.extend([-1173i16, -561, 0, 561, 1173, 561, 0, -561, 0].map(|x| x as u16));
        v.extend([0i16, 280, 501, 280, 0, -280, -501, -280, 0].map(|y| y as u16));
        v.extend([0b10000, 0b10000, 0, 0xFFFF]);
        v
    }

    #[test]
    fn rawler_directories_reach_the_crate_and_come_back_upright() {
        // A Canon note through the adapter, the camera held upright (orientation 6).
        let note = dir(&[short_tag(0x26, &five_d_mark_ii())]);
        let n = Notes {
            make: "canon",
            model: "Canon EOS 5D Mark II",
            ifds: &[],
            note: Some(&note),
            orientation: 6,
        };
        let f = Focus::from(note_focus(&n, None).unwrap());
        assert_eq!(f.points.len(), 9);
        let lit: Vec<_> = f
            .points
            .iter()
            .filter(|p| p.state != FocusState::Other)
            .collect();
        assert_eq!(lit.len(), 1);
        assert_eq!(lit[0].state, FocusState::InFocus);
        // The right point (X +1173 of 5616), turned a quarter clockwise: below the middle.
        let (cx, cy) = (lit[0].x + lit[0].w / 2.0, lit[0].y + lit[0].h / 2.0);
        assert!(
            close(cx, 0.5) && close(cy, 0.5 + 1173.0 / 5616.0),
            "{cx} {cy}"
        );
    }

    #[test]
    fn a_tiff_raw_has_its_maker_note_found() {
        // Root (Make, ExifIFD) -> EXIF (MakerNote) -> a plain Canon note
        // whose offsets are the file's, as a CR2 keeps it.
        let make: Entry = (MAKE, 2, 6, b"Canon\0".to_vec());
        let root = |exif_at: usize| {
            ifd_at(
                8,
                &[
                    make.clone(),
                    (EXIF_IFD, 4, 1, (exif_at as u32).to_le_bytes().to_vec()),
                ],
            )
        };
        let exif_at = 8 + root(0).len();
        // The EXIF directory holds one entry; its value, the note, follows it.
        let note_at = exif_at + 2 + 12 + 4;
        let note = ifd_at(note_at, &[short_tag(0x26, &five_d_mark_ii())]);
        let exif = ifd_at(exif_at, &[(MAKER_NOTE, 7, note.len() as u32, note)]);
        let mut file = b"II*\0".to_vec();
        file.extend(8u32.to_le_bytes());
        file.extend(root(exif_at));
        file.extend(exif);
        let f = of_file(&file, true).unwrap();
        assert_eq!(f.points.len(), 9);
        assert_eq!(
            f.points
                .iter()
                .filter(|p| p.state == focus_points::State::InFocus)
                .count(),
            1
        );
    }

    #[test]
    fn a_file_with_no_metadata_says_nothing() {
        assert!(of_file(b"not a photo at all, just some bytes", true).is_none());
        assert!(of_file(b"not a photo at all, just some bytes", false).is_none());
        assert!(of_file(&[], true).is_none());
    }

    /// What real files record, printed: `AG_RAWS` (';'-separated paths).
    /// `cargo test --lib focus_points::tests::real_files -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_files() {
        for path in std::env::var("AG_RAWS")
            .unwrap_or_default()
            .split(';')
            .filter(|p| !p.is_empty())
        {
            let focus = of_photo(path).unwrap();
            let used: Vec<_> = focus
                .iter()
                .flat_map(|f| &f.points)
                .filter(|p| p.state != FocusState::Other)
                .collect();
            println!(
                "{path}\n  mode {:?}, {} points, used {used:?}",
                focus.as_ref().and_then(|f| f.mode),
                focus.as_ref().map_or(0, |f| f.points.len())
            );
        }
    }

    #[test]
    fn the_switch_is_off_until_switched_on_and_then_kept() {
        let library = std::env::temp_dir().join(format!("ag-focus-points-{}", std::process::id()));
        std::fs::create_dir_all(&library).unwrap();
        assert!(!shown(&library));
        save_shown(&library, true).unwrap();
        assert!(shown(&library));
        save_shown(&library, false).unwrap();
        assert!(!shown(&library));
        std::fs::remove_dir_all(&library).ok();
    }

    #[test]
    fn the_answer_names_states_as_the_interface_does() {
        let f = Focus {
            mode: Some("Manual focus"),
            points: vec![FocusPoint {
                x: 0.1,
                y: 0.2,
                w: 0.3,
                h: 0.4,
                state: FocusState::InFocus,
            }],
        };
        let v = serde_json::to_value(&f).unwrap();
        assert_eq!(v["mode"], "Manual focus");
        assert_eq!(v["points"][0]["state"], "inFocus");
    }
}
