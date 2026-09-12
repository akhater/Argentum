//! Reading DNG camera profiles (`.dcp`).
//!
//! WHAT A PROFILE IS FOR
//!
//! rawler carries one colour matrix per camera, taken from Adobe's published
//! values — a generic characterisation of that model. It is good, and it is not
//! the same thing as a profile made by measuring a colour target through an
//! actual body. The remaining gap against darktable, after the sRAW levels fix,
//! is about 4-5% on red and blue, and this is what closes it.
//!
//! A `.dcp` is a TIFF file with no image in it: a header, one directory, and
//! tags holding matrices, an illuminant for each, and optionally a tone curve
//! and two lookup tables.
//!
//! WHAT THIS READS, AND WHAT IT IGNORES
//!
//! Matrices and illuminants only. `ProfileToneCurve`, `ProfileHueSatMapData`
//! and `ProfileLookTableData` are deliberately skipped for now: they are the
//! part that carries a *look* rather than a calibration, they are the fiddly
//! part to get right, and a half-implemented look table produces colour that is
//! differently wrong rather than better. The matrices are measurable on their
//! own — if they close the gap, the rest is a separate decision made against
//! numbers instead of hope.
//!
//! WHY COLORMATRIX AND NOT FORWARDMATRIX
//!
//! RawTherapee prefers `ForwardMatrix`, which maps camera RGB straight to XYZ.
//! It is the better path, but rawler's pipeline expects the DNG `ColorMatrix`
//! convention — XYZ to camera, the same direction as the values in its own
//! camera database — so substituting `ColorMatrix` is a drop-in and
//! `ForwardMatrix` would mean replacing the step around it too. Read both,
//! use `ColorMatrix`, and revisit once there is a measurement to justify it.
//!
//! Tag numbers are from the DNG specification, cross-checked against
//! RawTherapee's `rtengine/dcp.cc`.

use std::collections::HashMap;

/// DNG tags this reads. Names as the specification gives them.
mod tag {
    pub const UNIQUE_CAMERA_MODEL: u16 = 50708;
    pub const COLOR_MATRIX_1: u16 = 50721;
    pub const COLOR_MATRIX_2: u16 = 50722;
    pub const CALIBRATION_ILLUMINANT_1: u16 = 50778;
    pub const CALIBRATION_ILLUMINANT_2: u16 = 50779;
    pub const PROFILE_NAME: u16 = 50936;
    pub const FORWARD_MATRIX_1: u16 = 50964;
    pub const FORWARD_MATRIX_2: u16 = 50965;
}

/// A parsed profile. Matrices are row-major 3x3.
#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    pub name: Option<String>,
    /// The body the profile was made for, as the profile itself spells it.
    pub camera: Option<String>,
    /// EXIF light-source codes: 17 = Standard A, 21 = D65, 23 = D50.
    pub illuminant1: u16,
    pub illuminant2: Option<u16>,
    pub colour_matrix1: [f32; 9],
    pub colour_matrix2: Option<[f32; 9]>,
    pub forward_matrix1: Option<[f32; 9]>,
    pub forward_matrix2: Option<[f32; 9]>,
}

/// Everything that can go wrong, said plainly enough to act on.
#[derive(Debug, PartialEq)]
pub enum Error {
    TooShort,
    NotTiff,
    NoColourMatrix,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::TooShort => write!(f, "file is too short to be a DCP"),
            Error::NotTiff => write!(f, "not a TIFF-structured file"),
            Error::NoColourMatrix => write!(f, "no ColorMatrix1, so nothing to calibrate with"),
        }
    }
}

/// One tag's raw location, kept until we know how to read it.
struct Entry {
    kind: u16,
    count: u32,
    /// Offset of the value, already resolved past the inline/out-of-line rule.
    offset: usize,
}

/// Little- or big-endian, decided by the file's first two bytes.
#[derive(Clone, Copy)]
struct Endian(bool);

impl Endian {
    fn u16(self, b: &[u8], at: usize) -> Option<u16> {
        let raw = b.get(at..at + 2)?.try_into().ok()?;
        Some(if self.0 {
            u16::from_le_bytes(raw)
        } else {
            u16::from_be_bytes(raw)
        })
    }

    fn u32(self, b: &[u8], at: usize) -> Option<u32> {
        let raw = b.get(at..at + 4)?.try_into().ok()?;
        Some(if self.0 {
            u32::from_le_bytes(raw)
        } else {
            u32::from_be_bytes(raw)
        })
    }

    fn i32(self, b: &[u8], at: usize) -> Option<i32> {
        self.u32(b, at).map(|v| v as i32)
    }
}

/// Read a profile out of the bytes of a `.dcp` file.
pub fn parse(bytes: &[u8]) -> Result<Profile, Error> {
    if bytes.len() < 8 {
        return Err(Error::TooShort);
    }

    let endian = match &bytes[0..2] {
        b"II" => Endian(true),
        b"MM" => Endian(false),
        _ => return Err(Error::NotTiff),
    };

    // Magic is 42 for TIFF. Some profiles carry their own marker here, so this
    // does not insist on it — the directory that follows is what matters, and a
    // file that is not one will fail to yield a matrix anyway.
    let ifd_offset = endian.u32(bytes, 4).ok_or(Error::TooShort)? as usize;
    let entries = read_directory(bytes, endian, ifd_offset).ok_or(Error::NotTiff)?;

    let matrix = |t: u16| entries.get(&t).and_then(|e| read_matrix(bytes, endian, e));
    let colour_matrix1 = matrix(tag::COLOR_MATRIX_1).ok_or(Error::NoColourMatrix)?;

    Ok(Profile {
        name: entries
            .get(&tag::PROFILE_NAME)
            .and_then(|e| read_string(bytes, e)),
        camera: entries
            .get(&tag::UNIQUE_CAMERA_MODEL)
            .and_then(|e| read_string(bytes, e)),
        illuminant1: entries
            .get(&tag::CALIBRATION_ILLUMINANT_1)
            .and_then(|e| endian.u16(bytes, e.offset))
            .unwrap_or(0),
        illuminant2: entries
            .get(&tag::CALIBRATION_ILLUMINANT_2)
            .and_then(|e| endian.u16(bytes, e.offset)),
        colour_matrix1,
        colour_matrix2: matrix(tag::COLOR_MATRIX_2),
        forward_matrix1: matrix(tag::FORWARD_MATRIX_1),
        forward_matrix2: matrix(tag::FORWARD_MATRIX_2),
    })
}

/// Parse one IFD into tag -> entry.
fn read_directory(bytes: &[u8], endian: Endian, at: usize) -> Option<HashMap<u16, Entry>> {
    let count = endian.u16(bytes, at)? as usize;
    let mut out = HashMap::with_capacity(count);

    for i in 0..count {
        let e = at.checked_add(2 + i * 12)?;
        let tag = endian.u16(bytes, e)?;
        let kind = endian.u16(bytes, e + 2)?;
        let n = endian.u32(bytes, e + 4)?;

        // A value of four bytes or fewer sits in the entry; anything larger is
        // stored elsewhere and the entry holds its offset.
        let size = size_of_kind(kind).saturating_mul(n as usize);
        let offset = if size <= 4 {
            e + 8
        } else {
            endian.u32(bytes, e + 8)? as usize
        };

        out.insert(
            tag,
            Entry {
                kind,
                count: n,
                offset,
            },
        );
    }
    Some(out)
}

fn size_of_kind(kind: u16) -> usize {
    match kind {
        1 | 2 | 6 | 7 => 1, // BYTE, ASCII, SBYTE, UNDEFINED
        3 | 8 => 2,         // SHORT, SSHORT
        4 | 9 | 11 => 4,    // LONG, SLONG, FLOAT
        5 | 10 | 12 => 8,   // RATIONAL, SRATIONAL, DOUBLE
        _ => 1,
    }
}

/// Nine signed rationals, row-major.
fn read_matrix(bytes: &[u8], endian: Endian, e: &Entry) -> Option<[f32; 9]> {
    if e.count < 9 {
        return None;
    }
    let mut m = [0.0f32; 9];
    for (i, slot) in m.iter_mut().enumerate() {
        let at = e.offset.checked_add(i * 8)?;
        match e.kind {
            // SRATIONAL, which is what the specification calls for.
            10 => {
                let num = endian.i32(bytes, at)?;
                let den = endian.i32(bytes, at + 4)?;
                if den == 0 {
                    return None;
                }
                *slot = num as f32 / den as f32;
            }
            // RATIONAL, accepted because some writers use it and the values
            // are identical when they happen to be positive.
            5 => {
                let num = endian.u32(bytes, at)?;
                let den = endian.u32(bytes, at + 4)?;
                if den == 0 {
                    return None;
                }
                *slot = num as f32 / den as f32;
            }
            _ => return None,
        }
    }
    Some(m)
}

/// A NUL-terminated ASCII tag, trimmed.
fn read_string(bytes: &[u8], e: &Entry) -> Option<String> {
    let raw = bytes.get(e.offset..e.offset + e.count as usize)?;
    let text = raw.split(|&b| b == 0).next()?;
    let text = String::from_utf8_lossy(text).trim().to_string();
    (!text.is_empty()).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal little-endian DCP in memory.
    ///
    /// Writing one is the only honest way to test the reader without a profile
    /// on disk: it pins the format this claims to understand, and it fails if
    /// the offset arithmetic is wrong.
    fn synthetic() -> Vec<u8> {
        super::fixture::profile_for("Canon EOS 5D Mark II Test")
    }

    #[test]
    fn reads_a_profile() {
        let p = parse(&synthetic()).expect("should parse");
        assert_eq!(p.name.as_deref(), Some("Canon EOS 5D Mark II Test"));
        assert_eq!(p.camera.as_deref(), Some("Canon EOS 5D Mark II Test"));
        assert_eq!(p.illuminant1, 21);
        assert_eq!(p.illuminant2, None);
    }

    /// The matrix must come back in the order the file states it, not
    /// transposed — a transposed matrix still renders, just wrongly, which is
    /// exactly the kind of bug that survives a visual check.
    #[test]
    fn the_matrix_keeps_its_order() {
        let p = parse(&synthetic()).expect("should parse");
        assert!(
            (p.colour_matrix1[0] - 0.4716).abs() < 1e-6,
            "{:?}",
            p.colour_matrix1
        );
        assert!((p.colour_matrix1[2] + 0.0830).abs() < 1e-6);
        assert!((p.colour_matrix1[3] + 0.7798).abs() < 1e-6);
        assert!((p.colour_matrix1[8] - 0.6651).abs() < 1e-6);
    }

    /// A DNG ColorMatrix maps XYZ to camera, so a camera's response to white must
    /// be positive in every channel — a sensor cannot respond negatively to light.
    ///
    /// This replaced a row-sum range that was invented from synthetic data and
    /// promptly rejected a real Canon profile whose green row sums to 1.38. A weak
    /// property that is actually true beats a tight one that is not.
    fn responds_positively_to_white(m: &[f32; 9]) -> bool {
        const D50: [f32; 3] = [0.9642, 1.0, 0.8249];
        (0..3).all(|row| {
            let r: f32 = (0..3).map(|c| m[row * 3 + c] * D50[c]).sum();
            r > 0.0
        })
    }

    #[test]
    fn the_matrix_is_plausible() {
        let p = parse(&synthetic()).expect("should parse");
        assert!(
            responds_positively_to_white(&p.colour_matrix1),
            "not a camera matrix: {:?}",
            p.colour_matrix1
        );
    }

    #[test]
    fn rejects_what_is_not_a_profile() {
        assert_eq!(parse(b"nope").unwrap_err(), Error::TooShort);
        assert_eq!(parse(&[0u8; 64]).unwrap_err(), Error::NotTiff);
    }

    /// A TIFF with no ColorMatrix1 is readable and useless; say which.
    #[test]
    fn a_profile_without_a_matrix_is_refused() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"II");
        bytes.extend_from_slice(&42u16.to_le_bytes());
        bytes.extend_from_slice(&8u32.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes()); // zero entries
        bytes.extend_from_slice(&0u32.to_le_bytes());
        assert_eq!(parse(&bytes).unwrap_err(), Error::NoColourMatrix);
    }

    /// Truncation must not panic. Profiles come from outside the app.
    #[test]
    fn truncation_never_panics() {
        let full = synthetic();
        for cut in 0..full.len() {
            let _ = parse(&full[..cut]);
        }
    }
}

#[cfg(test)]
mod dump {
    /// Print a real profile's contents. Used to replace invented test numbers
    /// with the published ones.
    #[test]
    #[ignore = "reads an installed profile; run by hand"]
    fn show_an_installed_profile() {
        let path = std::env::var("AG_DCP").expect("set AG_DCP to a .dcp");
        let bytes = std::fs::read(&path).expect("read");
        let p = super::parse(&bytes).expect("parse");
        println!("name       {:?}", p.name);
        println!("camera     {:?}", p.camera);
        println!("illum      {} / {:?}", p.illuminant1, p.illuminant2);
        println!("colour1    {:?}", p.colour_matrix1);
        println!("colour2    {:?}", p.colour_matrix2);
        println!("forward1   {:?}", p.forward_matrix1);
        println!("forward2   {:?}", p.forward_matrix2);
    }
}

/// Write a deliberately wrong profile, for testing that one is applied at all.
///
/// A real camera profile moves the average pixel about two levels out of 255,
/// which is invisible — so "is it even being used?" is a fair question that no
/// amount of arithmetic settles. This makes one whose effect nobody can miss.
///
/// It swaps the red and blue *columns* of the ForwardMatrix. Column swapping
/// leaves every row sum untouched, so the matrix still takes neutral camera RGB
/// to D50 white: white balance keeps working, greys stay grey, and only
/// saturated colour moves — which is exactly the part of the picture a camera
/// matrix governs. Warm becomes cool. If selecting this does nothing, the
/// plumbing is broken; if it does something, the plumbing is fine and a real
/// profile is simply a small correction.
#[cfg(test)]
mod make_a_test_profile {
    use super::tag;

    fn ascii(tag: u16, text: &str) -> (u16, u16, Vec<u8>) {
        let mut bytes = text.as_bytes().to_vec();
        bytes.push(0);
        (tag, 2, bytes)
    }

    fn srational(tag: u16, values: &[f32; 9]) -> (u16, u16, Vec<u8>) {
        let mut bytes = Vec::with_capacity(72);
        for v in values {
            let numerator = (v * 10_000.0).round() as i32;
            bytes.extend_from_slice(&numerator.to_le_bytes());
            bytes.extend_from_slice(&10_000i32.to_le_bytes());
        }
        (tag, 10, bytes)
    }

    #[test]
    #[ignore = "writes a .dcp into a library; run by hand"]
    fn write_a_swapped_profile() {
        let out = std::env::var("AG_OUT").expect("set AG_OUT to the .dcp to write");
        let camera =
            std::env::var("AG_CAMERA").unwrap_or_else(|_| "Canon EOS 5D Mark II".to_string());

        // The published Canon D50 pair, as read from RawTherapee's profile.
        let colour: [f32; 9] = [
            0.5957, -0.0667, -0.0863, -0.5129, 1.3024, 0.2313, -0.0577, 0.12, 0.6706,
        ];
        let forward: [f32; 9] = [
            0.6399, 0.1294, 0.1949, 0.2827, 0.6579, 0.0594, 0.0001, 0.0051, 0.8199,
        ];

        // Swap camera red and blue: columns 0 and 2 of each row.
        let mut swapped = forward;
        for row in 0..3 {
            swapped.swap(row * 3, row * 3 + 2);
        }

        // Row sums must be unchanged, or neutral would stop being neutral and
        // this would test white balance rather than the profile.
        for row in 0..3 {
            let before: f32 = forward[row * 3..row * 3 + 3].iter().sum();
            let after: f32 = swapped[row * 3..row * 3 + 3].iter().sum();
            assert!(
                (before - after).abs() < 1e-6,
                "row {row} changed its white point"
            );
        }

        let fields: Vec<(u16, u16, Vec<u8>)> = vec![
            ascii(tag::PROFILE_NAME, "TEST - red and blue swapped"),
            ascii(tag::UNIQUE_CAMERA_MODEL, &camera),
            (
                tag::CALIBRATION_ILLUMINANT_1,
                3,
                23u16.to_le_bytes().to_vec(),
            ), // D50
            srational(tag::COLOR_MATRIX_1, &colour),
            srational(tag::FORWARD_MATRIX_1, &swapped),
        ];

        let ifd_at = 8usize;
        let heap_at = ifd_at + 2 + fields.len() * 12 + 4;

        let mut heap = Vec::new();
        let mut offsets = Vec::new();
        for (_, _, value) in &fields {
            offsets.push((heap_at + heap.len()) as u32);
            heap.extend_from_slice(value);
        }

        let mut file = Vec::new();
        file.extend_from_slice(b"II");
        file.extend_from_slice(&42u16.to_le_bytes());
        file.extend_from_slice(&(ifd_at as u32).to_le_bytes());
        file.extend_from_slice(&(fields.len() as u16).to_le_bytes());

        for (i, (tag_id, kind, value)) in fields.iter().enumerate() {
            let count = if *kind == 10 { 9 } else { value.len() } as u32;
            file.extend_from_slice(&tag_id.to_le_bytes());
            file.extend_from_slice(&kind.to_le_bytes());
            file.extend_from_slice(&count.to_le_bytes());
            if value.len() <= 4 {
                let mut inline = value.clone();
                inline.resize(4, 0);
                file.extend_from_slice(&inline);
            } else {
                file.extend_from_slice(&offsets[i].to_le_bytes());
            }
        }
        file.extend_from_slice(&0u32.to_le_bytes());
        file.extend_from_slice(&heap);

        // It has to survive our own reader, or it is not a test of anything.
        let parsed = super::parse(&file).expect("the written profile must parse");
        assert_eq!(parsed.camera.as_deref(), Some(camera.as_str()));
        assert!(parsed.forward_matrix1.is_some());

        std::fs::write(&out, &file).expect("write");
        println!("wrote {out}");
        println!("  name    {:?}", parsed.name);
        println!("  camera  {:?}", parsed.camera);
        println!("  forward {:?}", parsed.forward_matrix1.unwrap());
    }
}

/// A minimal little-endian DCP, built in memory, for tests here and in
/// `profiles`.
///
/// Writing one is the only honest way to test the reader without a profile on
/// disk: it pins the format this claims to understand, and it fails if the
/// offset arithmetic is wrong. It takes the camera name because `profiles`
/// matches on that, so its tests need profiles for two different bodies.
#[cfg(test)]
pub(crate) mod fixture {
    use super::tag;

    /// The same profile with the forward matrix left out.
    ///
    /// A real shape: the DNG spec requires `ColorMatrix1` and makes
    /// `ForwardMatrix1` optional, so profiles like this exist and get handed to
    /// us. Argentum cannot render with one, and this is what proves it says so
    /// rather than accepting it and doing nothing.
    pub fn colour_matrix_only(camera: &str) -> Vec<u8> {
        build(camera, false)
    }

    pub fn profile_for(camera: &str) -> Vec<u8> {
        build(camera, true)
    }

    fn build(camera: &str, with_forward: bool) -> Vec<u8> {
        let matrix: [(i32, i32); 9] = [
            (4716, 10000),
            (603, 10000),
            (-830, 10000),
            (-7798, 10000),
            (15474, 10000),
            (2480, 10000),
            (-1496, 10000),
            (1937, 10000),
            (6651, 10000),
        ];
        // TIFF ASCII values are NUL-terminated. Built by hand rather than
        // with an escape, because a literal NUL in this file is easy to write
        // by accident and impossible to see.
        let mut name = camera.as_bytes().to_vec();
        name.push(0);
        let name = name.as_slice();

        // A real profile carries a forward matrix, and without one Argentum
        // refuses it — so a fixture without one is not a fixture of anything
        // this code will ever be handed. These are the published Canon EOS 5D
        // Mark II values, so neutral in gives D50 white out.
        let forward: [(i32, i32); 9] = [
            (6399, 10000),
            (1294, 10000),
            (1949, 10000),
            (2827, 10000),
            (6579, 10000),
            (594, 10000),
            (1, 10000),
            (51, 10000),
            (8199, 10000),
        ];

        let mut entries: Vec<(u16, u16, u32)> = vec![
            (tag::PROFILE_NAME, 2, name.len() as u32),
            (tag::UNIQUE_CAMERA_MODEL, 2, name.len() as u32),
            (tag::CALIBRATION_ILLUMINANT_1, 3, 1),
            (tag::COLOR_MATRIX_1, 10, 9),
        ];
        if with_forward {
            entries.push((tag::FORWARD_MATRIX_1, 10, 9));
        }
        let entries = entries;

        let ifd_at = 8usize;
        let heap_at = ifd_at + 2 + entries.len() * 12 + 4;

        let mut heap: Vec<u8> = Vec::new();
        let mut offsets: Vec<u32> = Vec::new();
        for (t, _, _) in &entries {
            offsets.push((heap_at + heap.len()) as u32);
            match *t {
                tag::PROFILE_NAME | tag::UNIQUE_CAMERA_MODEL => heap.extend_from_slice(name),
                tag::COLOR_MATRIX_1 => {
                    for (n, d) in matrix {
                        heap.extend_from_slice(&n.to_le_bytes());
                        heap.extend_from_slice(&d.to_le_bytes());
                    }
                }
                tag::FORWARD_MATRIX_1 => {
                    for (n, d) in forward {
                        heap.extend_from_slice(&n.to_le_bytes());
                        heap.extend_from_slice(&d.to_le_bytes());
                    }
                }
                _ => {}
            }
        }

        let mut out = Vec::new();
        out.extend_from_slice(b"II");
        out.extend_from_slice(&42u16.to_le_bytes());
        out.extend_from_slice(&(ifd_at as u32).to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());

        for (i, (t, kind, count)) in entries.iter().enumerate() {
            out.extend_from_slice(&t.to_le_bytes());
            out.extend_from_slice(&kind.to_le_bytes());
            out.extend_from_slice(&count.to_le_bytes());
            if *t == tag::CALIBRATION_ILLUMINANT_1 {
                out.extend_from_slice(&21u16.to_le_bytes()); // D65, inline
                out.extend_from_slice(&0u16.to_le_bytes());
            } else {
                out.extend_from_slice(&offsets[i].to_le_bytes());
            }
        }
        out.extend_from_slice(&0u32.to_le_bytes()); // no next IFD
        out.extend_from_slice(&heap);
        out
    }
}
