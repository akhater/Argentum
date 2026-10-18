//! The JPEGs a camera stores inside its RAW, found without decoding anything.
//!
//! Every camera keeps at least one beside the sensor data: a tiny thumbnail,
//! a screen-sized preview, often a full-size one. A grid thumbnail is made
//! from the smallest that is still big enough, so a RAW is never developed
//! for the grid, and a full-size JPEG is decoded only when nothing smaller
//! will do.
//!
//! Three layouts are read here: TIFF-based RAWs (CR2, NEF, ARW, DNG, PEF,
//! RW2 and most others), Canon's CR3 and Fujifilm's RAF. `thumbnail.rs` asks
//! rawler for anything else. Which way up the picture goes is read here too,
//! from the same tag rawler reads it from, without handing rawler the file:
//! rawler copies a whole RAW into memory before it reads a byte of it.

/// One embedded JPEG, and its size read from its own header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Embedded<'a> {
    pub jpeg: &'a [u8],
    pub width: u32,
    pub height: u32,
}

/// Every embedded JPEG the image crate can decode, each once.
pub fn jpegs(file: &[u8]) -> Vec<Embedded<'_>> {
    let mut ranges = rr_tiff_jpegs(file);
    ranges.extend(ag_cr3_preview(file));
    ranges.extend(ag_raf_preview(file));
    ranges.sort_unstable();
    ranges.dedup();
    ranges
        .into_iter()
        .filter_map(|(at, len)| file.get(at as usize..(at as usize).checked_add(len as usize)?))
        .filter_map(|jpeg| {
            let (width, height) = ag_jpeg_size(jpeg)?;
            Some(Embedded {
                jpeg,
                width,
                height,
            })
        })
        .collect()
}

/// Which way up the camera held it, as EXIF numbers it (1 to 8): the
/// Orientation tag (274) of a TIFF-based RAW's first IFD, or of a CR3's
/// CMT1 box. None when the file does not say.
pub fn orientation(file: &[u8]) -> Option<u16> {
    ag_tiff_orientation(file).or_else(|| ag_cr3_orientation(file))
}

/// The order to try them in for a thumbnail `px` on its long edge: the
/// smallest at least that big first, larger ones after; then those too
/// small, largest first.
pub fn by_preference(mut found: Vec<Embedded<'_>>, px: u32) -> Vec<Embedded<'_>> {
    found.sort_by_key(|e| {
        let long = e.width.max(e.height);
        if long >= px {
            (false, long)
        } else {
            (true, u32::MAX - long)
        }
    });
    found
}

/// The width and height in a JPEG's frame header, for the kinds the image
/// crate decodes (baseline, extended, progressive). None for anything else,
/// which keeps out the lossless JPEG some RAWs store their sensor data in.
fn ag_jpeg_size(jpeg: &[u8]) -> Option<(u32, u32)> {
    if jpeg.get(..2)? != [0xFF, 0xD8] {
        return None;
    }
    let mut at = 2;
    loop {
        if *jpeg.get(at)? != 0xFF {
            return None;
        }
        let marker = *jpeg.get(at + 1)?;
        match marker {
            // Fill byte.
            0xFF => at += 1,
            // Markers that stand alone.
            0x01 | 0xD0..=0xD7 => at += 2,
            // Start of scan, end of image: no frame header before them.
            0xD9 | 0xDA => return None,
            0xC0..=0xC2 => {
                let height = u16::from_be_bytes([*jpeg.get(at + 5)?, *jpeg.get(at + 6)?]);
                let width = u16::from_be_bytes([*jpeg.get(at + 7)?, *jpeg.get(at + 8)?]);
                return (width > 0 && height > 0).then_some((width as u32, height as u32));
            }
            // Lossless, hierarchical and arithmetic frames.
            0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF => return None,
            _ => {
                let len = u16::from_be_bytes([*jpeg.get(at + 2)?, *jpeg.get(at + 3)?]);
                at += 2 + len as usize;
            }
        }
    }
}

// rr_: RapidRAW src-tauri/src/image_loader.rs largest_tiff_jpeg_preview,
// v1.6.5 (79c2a46b). Changed here: it returns every candidate instead of
// decoding the largest; it also reads Panasonic's and Olympus's TIFF
// signatures and Panasonic's JpgFromRaw tag (46).
fn rr_tiff_jpegs(buf: &[u8]) -> Vec<(u64, u64)> {
    let le = match buf.get(..4) {
        Some([0x49, 0x49, 0x2A, 0x00]) => true,
        Some([0x4D, 0x4D, 0x00, 0x2A]) => false,
        // Panasonic RW2; Olympus ORF.
        Some([0x49, 0x49, 0x55, 0x00] | [0x49, 0x49, 0x52, 0x4F] | [0x49, 0x49, 0x52, 0x53]) => {
            true
        }
        Some([0x4D, 0x4D, 0x4F, 0x52]) => false,
        _ => return Vec::new(),
    };
    let rd16 = |o: usize| -> Option<u64> {
        let b: [u8; 2] = buf.get(o..o + 2)?.try_into().ok()?;
        Some(if le {
            u16::from_le_bytes(b)
        } else {
            u16::from_be_bytes(b)
        } as u64)
    };
    let rd32 = |o: usize| -> Option<u64> {
        let b: [u8; 4] = buf.get(o..o + 4)?.try_into().ok()?;
        Some(if le {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        } as u64)
    };

    let mut candidates: Vec<(u64, u64)> = Vec::new();
    let Some(first) = rd32(4) else {
        return candidates;
    };
    let mut queue: Vec<u64> = vec![first];
    let mut seen = std::collections::HashSet::new();

    while let Some(ifd) = queue.pop() {
        if !seen.insert(ifd) || seen.len() > 64 {
            continue;
        }
        let Some(n) = rd16(ifd as usize) else {
            continue;
        };

        let mut compression: u64 = 0;
        let mut strip: Option<(u64, u64)> = None;
        let mut old_jpeg: Option<(u64, u64)> = None;

        for i in 0..n {
            let e = ifd as usize + 2 + (i as usize) * 12;
            let (Some(tag), Some(count), Some(val)) = (rd16(e), rd32(e + 4), rd32(e + 8)) else {
                continue;
            };
            match tag {
                // ag_: Panasonic's JpgFromRaw, an undefined-type blob.
                46 => candidates.push((val, count)),
                259 => compression = val,
                273 if count == 1 => strip = Some((val, strip.map_or(0, |s| s.1))),
                279 if count == 1 => strip = strip.map(|s| (s.0, val)).or(Some((0, val))),
                513 => old_jpeg = Some((val, old_jpeg.map_or(0, |s| s.1))),
                514 => old_jpeg = old_jpeg.map(|s| (s.0, val)).or(Some((0, val))),
                330 => {
                    if count == 1 {
                        queue.push(val);
                    } else {
                        for j in 0..count.min(8) {
                            if let Some(p) = rd32(val as usize + (j as usize) * 4) {
                                queue.push(p);
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        if matches!(compression, 6 | 7)
            && let Some(s) = strip
        {
            candidates.push(s);
        }
        if let Some(oj) = old_jpeg {
            candidates.push(oj);
        }
        if let Some(next) = rd32(ifd as usize + 2 + (n as usize) * 12)
            && next != 0
        {
            queue.push(next);
        }
    }
    candidates
}

fn be32(file: &[u8], at: usize) -> Option<u64> {
    Some(u32::from_be_bytes(file.get(at..at + 4)?.try_into().ok()?) as u64)
}

/// One ISO media box: its type, where its contents start, and its end.
struct Bmff {
    kind: [u8; 4],
    body: usize,
    end: usize,
}

/// The boxes between `from` and `to`, in order, stopping at a broken one.
fn boxes(file: &[u8], from: usize, to: usize) -> impl Iterator<Item = Bmff> + '_ {
    let mut at = from;
    std::iter::from_fn(move || {
        if at + 8 > to {
            return None;
        }
        let (size, header) = match be32(file, at)? {
            1 => (
                u64::from_be_bytes(file.get(at + 8..at + 16)?.try_into().ok()?),
                16,
            ),
            0 => ((to - at) as u64, 8),
            size => (size, 8),
        };
        let end = at.checked_add(usize::try_from(size).ok()?)?;
        if size < header as u64 || end > to {
            return None;
        }
        let found = Bmff {
            kind: file.get(at + 4..at + 8)?.try_into().ok()?,
            body: at + header,
            end,
        };
        at = end;
        Some(found)
    })
}

/// The `uuid` box with this id among `from..to`, as the part after the id.
fn uuid_box(file: &[u8], from: usize, to: usize, id: &[u8; 16]) -> Option<Bmff> {
    boxes(file, from, to)
        .find(|b| &b.kind == b"uuid" && file.get(b.body..b.body + 16) == Some(&id[..]))
        .map(|b| Bmff {
            body: b.body + 16,
            ..b
        })
}

fn is_cr3(file: &[u8]) -> bool {
    file.get(4..12) == Some(&b"ftypcrx "[..])
}

/// CR3's screen-sized preview (1620 x 1080 on most bodies): a `PRVW` box
/// inside a top-level `uuid` box, after Laurent Clévy's description of the
/// format (github.com/lclevy/canon_cr3). The full-size JPEG CR3s also carry
/// is left to rawler, for files without this one.
fn ag_cr3_preview(file: &[u8]) -> Option<(u64, u64)> {
    const PREVIEW: [u8; 16] = [
        0xEA, 0xF4, 0x2B, 0x5E, 0x1C, 0x98, 0x4B, 0x88, 0xB9, 0xFB, 0xB7, 0xDC, 0x40, 0x6E, 0x4D,
        0x16,
    ];
    if !is_cr3(file) {
        return None;
    }
    let preview = uuid_box(file, 0, file.len(), &PREVIEW)?;
    // 8 bytes, then PRVW: size, tag, 4 + 2 unknown bytes, width, height, 2
    // more, the JPEG's length, then the JPEG.
    let search = file.get(preview.body..(preview.body + 64).min(preview.end))?;
    let prvw = preview.body
        + search
            .windows(4)
            .position(|w| w == b"PRVW")?
            .checked_sub(4)?;
    Some(((prvw + 24) as u64, be32(file, prvw + 20)?))
}

/// CR3's orientation: in the TIFF IFD of the CMT1 box, inside the Canon
/// `uuid` box in `moov` (same description).
fn ag_cr3_orientation(file: &[u8]) -> Option<u16> {
    const CANON: [u8; 16] = [
        0x85, 0xC0, 0xB6, 0x87, 0x82, 0x0F, 0x11, 0xE0, 0x81, 0x11, 0xF4, 0xCE, 0x46, 0x2B, 0x6A,
        0x48,
    ];
    if !is_cr3(file) {
        return None;
    }
    let moov = boxes(file, 0, file.len()).find(|b| &b.kind == b"moov")?;
    let canon = uuid_box(file, moov.body, moov.end, &CANON)?;
    let cmt1 = boxes(file, canon.body, canon.end).find(|b| &b.kind == b"CMT1")?;
    ag_tiff_orientation(file.get(cmt1.body..cmt1.end)?)
}

/// The Orientation tag (274) in a TIFF's first IFD.
fn ag_tiff_orientation(tiff: &[u8]) -> Option<u16> {
    let le = match tiff.get(..2)? {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    let rd16 = |o: usize| -> Option<u16> {
        let b: [u8; 2] = tiff.get(o..o + 2)?.try_into().ok()?;
        Some(if le {
            u16::from_le_bytes(b)
        } else {
            u16::from_be_bytes(b)
        })
    };
    let rd32 = |o: usize| -> Option<u32> {
        let b: [u8; 4] = tiff.get(o..o + 4)?.try_into().ok()?;
        Some(if le {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        })
    };
    let ifd = rd32(4)? as usize;
    (0..rd16(ifd)? as usize)
        .map(|i| ifd + 2 + i * 12)
        // A SHORT sits in the first two bytes of the entry's value.
        .find(|&e| rd16(e) == Some(274))
        .and_then(|e| rd16(e + 8))
        .filter(|o| (1..=8).contains(o))
}

/// RAF's JPEG: its offset and length sit at fixed places in the header.
fn ag_raf_preview(file: &[u8]) -> Option<(u64, u64)> {
    if file.get(..16)? != b"FUJIFILMCCD-RAW " {
        return None;
    }
    Some((be32(file, 84)?, be32(file, 88)?))
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    /// A real JPEG of `w` x `h`, made by the image crate.
    pub fn jpeg(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbImage::from_fn(w, h, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, 90])
        });
        let mut out = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 80)
            .encode_image(&img)
            .unwrap();
        out
    }

    /// A little-endian TIFF with one IFD per JPEG, each as JPEGInterchangeFormat.
    pub fn tiff_with(jpegs: &[Vec<u8>]) -> Vec<u8> {
        tiff(None, jpegs)
    }

    /// The same, its first IFD saying which way up (tag 274) when given.
    pub fn tiff(orientation: Option<u16>, jpegs: &[Vec<u8>]) -> Vec<u8> {
        let entries = |i: usize| {
            if i == 0 && orientation.is_some() {
                3
            } else {
                2
            }
        };
        let ifd_len = |i: usize| 2 + entries(i) * 12 + 4;
        let mut out = b"II\x2A\x00".to_vec();
        out.extend(8u32.to_le_bytes());
        let ifds = jpegs.len().max(1);
        let mut ifd_at = 8;
        let mut data_at = 8 + (0..ifds).map(ifd_len).sum::<usize>();
        for i in 0..ifds {
            let j = jpegs.get(i);
            let count = if j.is_some() {
                entries(i)
            } else {
                entries(i) - 2
            };
            out.extend((count as u16).to_le_bytes());
            let mut entry = |tag: u16, kind: u16, val: u32| {
                out.extend(tag.to_le_bytes());
                out.extend(kind.to_le_bytes());
                out.extend(1u32.to_le_bytes());
                out.extend(val.to_le_bytes());
            };
            if let (0, Some(o)) = (i, orientation) {
                entry(274, 3, o as u32);
            }
            if let Some(j) = j {
                entry(513, 4, data_at as u32);
                entry(514, 4, j.len() as u32);
                data_at += j.len();
            }
            ifd_at += 2 + count * 12 + 4;
            let next = if i + 1 < ifds { ifd_at as u32 } else { 0 };
            out.extend(next.to_le_bytes());
        }
        for j in jpegs {
            out.extend(j);
        }
        out
    }

    /// A CR3's boxes, as far as this file reads them: its preview, and its
    /// orientation in CMT1.
    pub fn cr3(preview: &[u8], orientation: u16) -> Vec<u8> {
        let boxed = |kind: &[u8], body: &[u8]| {
            let mut b = ((8 + body.len()) as u32).to_be_bytes().to_vec();
            b.extend(kind);
            b.extend(body);
            b
        };
        let mut canon = vec![
            0x85, 0xC0, 0xB6, 0x87, 0x82, 0x0F, 0x11, 0xE0, 0x81, 0x11, 0xF4, 0xCE, 0x46, 0x2B,
            0x6A, 0x48,
        ];
        canon.extend(boxed(b"CNCV", b"CanonCR3_001/00.09.00/00.00.00"));
        canon.extend(boxed(b"CMT1", &tiff(Some(orientation), &[])));
        let moov = boxed(b"moov", &boxed(b"uuid", &canon));
        let mut prvw = vec![0, 0, 0, 0, 0, 1];
        prvw.extend(1620u16.to_be_bytes());
        prvw.extend(1080u16.to_be_bytes());
        prvw.extend([0, 1]);
        prvw.extend((preview.len() as u32).to_be_bytes());
        prvw.extend(preview);
        let mut uuid = vec![
            0xEA, 0xF4, 0x2B, 0x5E, 0x1C, 0x98, 0x4B, 0x88, 0xB9, 0xFB, 0xB7, 0xDC, 0x40, 0x6E,
            0x4D, 0x16, 0, 0, 0, 0, 0, 0, 0, 1,
        ];
        uuid.extend(boxed(b"PRVW", &prvw));
        let mut file = boxed(b"ftyp", b"crx \x00\x00\x00\x01crx isom");
        file.extend(moov);
        file.extend(boxed(b"uuid", &uuid));
        file
    }

    #[test]
    fn a_jpegs_size_comes_from_its_header() {
        assert_eq!(ag_jpeg_size(&jpeg(64, 48)), Some((64, 48)));
        assert_eq!(ag_jpeg_size(b"not a jpeg"), None);
        // Lossless (SOF3), as CR2 and DNG keep sensor data: not a picture.
        let lossless = [
            0xFF, 0xD8, 0xFF, 0xC3, 0x00, 0x0B, 8, 0, 16, 0, 16, 1, 0, 0, 0,
        ];
        assert_eq!(ag_jpeg_size(&lossless), None);
    }

    #[test]
    fn every_jpeg_in_a_tiff_is_found_and_the_smallest_big_enough_comes_first() {
        let file = tiff_with(&[jpeg(160, 120), jpeg(1600, 1067), jpeg(640, 427)]);
        let found = jpegs(&file);
        let sizes = |v: &[Embedded]| v.iter().map(|e| (e.width, e.height)).collect::<Vec<_>>();
        assert_eq!(found.len(), 3);
        assert_eq!(
            sizes(&by_preference(found.clone(), 400)),
            [(640, 427), (1600, 1067), (160, 120)]
        );
        // Nothing big enough: the largest there is.
        assert_eq!(sizes(&by_preference(found, 2000))[0], (1600, 1067));
    }

    #[test]
    fn a_cr3_preview_box_and_its_orientation_are_found() {
        let preview = jpeg(1620, 1080);
        let file = cr3(&preview, 8);
        let found = jpegs(&file);
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].width, found[0].height), (1620, 1080));
        assert_eq!(found[0].jpeg, &preview[..]);
        assert_eq!(orientation(&file), Some(8));
    }

    #[test]
    fn a_tiff_raw_says_which_way_up_in_its_first_ifd() {
        assert_eq!(orientation(&tiff(Some(6), &[jpeg(64, 48)])), Some(6));
        assert_eq!(orientation(&tiff(None, &[jpeg(64, 48)])), None);
        // Out of EXIF's range: as if it said nothing.
        assert_eq!(orientation(&tiff(Some(9), &[jpeg(64, 48)])), None);
    }

    #[test]
    fn a_raf_preview_is_found() {
        let preview = jpeg(320, 240);
        let mut file = b"FUJIFILMCCD-RAW 0201FF383501".to_vec();
        file.resize(100, 0);
        file[84..88].copy_from_slice(&100u32.to_be_bytes());
        file[88..92].copy_from_slice(&(preview.len() as u32).to_be_bytes());
        file.extend(&preview);
        assert_eq!(jpegs(&file).len(), 1);
    }

    #[test]
    fn broken_files_find_nothing_and_do_not_panic() {
        for file in [
            &b""[..],
            b"II\x2A\x00",
            b"II\x2A\x00\xFF\xFF\xFF\xFF",
            b"MM\x00\x2A\x00\x00\x00\x08\xFF\xFF",
            b"\x00\x00\x00\x18ftypcrx \x00\x00\x00\x01",
            b"FUJIFILMCCD-RAW ",
        ] {
            assert!(jpegs(file).is_empty());
            assert!(orientation(file).is_none());
        }
    }
}
