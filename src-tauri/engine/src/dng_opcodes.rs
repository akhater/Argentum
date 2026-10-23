//! A DNG's opcode lists: the raw image's OpcodeList1, 2 or 3 as stored, and
//! the opcodes in one. Shared by the gain maps (`dng_gain_map.rs`) and the
//! lens corrections (`rm_lens_builtin.rs`); each reads only the opcodes it
//! applies.
//!
//! The list's layout is the DNG Specification's (1.7.1.0, chapter 7), as
//! RAWmakase reads it (`crates/rawmakase-raw/src/container/dng/opcodes.rs`
//! @ a8c5d6d7, MIT): a big-endian count, then per opcode its ID, version,
//! flags, parameter size and parameters.

use rawler::formats::tiff::reader::TiffReader;
use rawler::formats::tiff::{GenericTiffReader, IFD, Value};
use rawler::tags::{DngTag, TiffCommonTag};

/// The GainMap opcode's ID: OpcodeList2's are the mosaic's
/// (`dng_gain_map.rs`), OpcodeList3's the demosaiced image's
/// (`rm_lens_builtin.rs`).
pub(crate) const GAIN_MAP: u32 = 9;

/// The opcode list `tag` of a DNG's raw image, as stored. `None` for a file
/// that is not a DNG or has no such list.
pub(crate) fn list(file: &[u8], tag: DngTag) -> Option<Vec<u8>> {
    let tiff = dng(file)?;
    match &raw_ifd(&tiff, tag)?.get_entry(tag)?.value {
        Value::Undefined(bytes) | Value::Byte(bytes) => Some(bytes.clone()),
        _ => None,
    }
}

/// The size, [height, width] in pixels, of the image the raw image's opcode
/// list `tag` applies to: its active area, or the whole stored image, as
/// RAWmakase reads it (`crates/rawmakase-model/src/dng.rs` @ f0899e0, MIT).
pub(crate) fn image_size(file: &[u8], tag: DngTag) -> Option<[u32; 2]> {
    let tiff = dng(file)?;
    let ifd = raw_ifd(&tiff, tag)?;
    let numbers = |tag: u16| -> Option<Vec<u32>> {
        let v = &ifd.get_entry(tag)?.value;
        (0..v.count())
            .map(|i| v.get_u32(i).ok().flatten())
            .collect()
    };
    match numbers(DngTag::ActiveArea as u16).as_deref() {
        Some(&[top, left, bottom, right]) if bottom > top && right > left => {
            Some([bottom - top, right - left])
        }
        _ => {
            let dim = |tag: TiffCommonTag| Some(*numbers(tag as u16)?.first()?);
            Some([
                dim(TiffCommonTag::ImageLength)?,
                dim(TiffCommonTag::ImageWidth)?,
            ])
        }
    }
}

/// The file as a TIFF, when it is a DNG.
fn dng(file: &[u8]) -> Option<GenericTiffReader> {
    if file.len() < 8 || !(file.starts_with(b"II") || file.starts_with(b"MM")) {
        return None;
    }
    let tiff = GenericTiffReader::new_with_buffer(file, 0, 0, None).ok()?;
    tiff.root_ifd().get_entry(DngTag::DNGVersion)?;
    Some(tiff)
}

/// The raw image's directory holding `tag`, not a reduced copy's.
fn raw_ifd(tiff: &GenericTiffReader, tag: DngTag) -> Option<&IFD> {
    tiff.find_ifds_with_tag(tag).into_iter().find(|ifd| {
        ifd.get_entry(TiffCommonTag::NewSubFileType)
            .is_none_or(|e| e.force_u32(0) & 1 == 0)
    })
}

/// One opcode of a list.
pub(crate) struct Opcode<'a> {
    pub(crate) id: u32,
    pub(crate) version: u32,
    pub(crate) params: &'a [u8],
}

/// The list's opcodes in order; `None` when it does not parse.
pub(crate) fn opcodes(list: &[u8]) -> Option<Vec<Opcode<'_>>> {
    if list.is_empty() {
        return Some(Vec::new());
    }
    let mut p = Params { data: list, at: 0 };
    let count = p.u32()?;
    let mut out = Vec::new();
    for _ in 0..count {
        let id = p.u32()?;
        let version = p.u32()?;
        let _flags = p.u32()?;
        let size = p.usize()?;
        let params = p.take(size)?;
        out.push(Opcode {
            id,
            version,
            params,
        });
    }
    Some(out)
}

/// Big-endian reads of an opcode's parameters.
pub(crate) struct Params<'a> {
    pub(crate) data: &'a [u8],
    pub(crate) at: usize,
}

impl<'a> Params<'a> {
    pub(crate) fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(n)?;
        let bytes = self.data.get(self.at..end)?;
        self.at = end;
        Some(bytes)
    }
    pub(crate) fn u32(&mut self) -> Option<u32> {
        let b = self.take(4)?;
        Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
    pub(crate) fn f32(&mut self) -> Option<f32> {
        self.u32().map(f32::from_bits)
    }
    pub(crate) fn f64(&mut self) -> Option<f64> {
        let mut v = [0u8; 8];
        v.copy_from_slice(self.take(8)?);
        Some(f64::from_be_bytes(v))
    }
    pub(crate) fn usize(&mut self) -> Option<usize> {
        usize::try_from(self.u32()?).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_that_is_not_a_dng_has_no_list() {
        assert!(list(b"not a tiff", DngTag::OpcodeList2).is_none());
        assert!(list(&[], DngTag::OpcodeList3).is_none());
    }

    #[test]
    fn a_list_reads_in_order_and_a_short_one_does_not() {
        let mut list = 2u32.to_be_bytes().to_vec();
        for (id, params) in [(9u32, vec![1u8, 2]), (3, vec![3])] {
            for v in [id, 0x0103_0000, 0, params.len() as u32] {
                list.extend(v.to_be_bytes());
            }
            list.extend(params);
        }
        let ops = opcodes(&list).unwrap();
        assert_eq!(
            ops.iter().map(|o| (o.id, o.params)).collect::<Vec<_>>(),
            [(9, &[1u8, 2][..]), (3, &[3][..])]
        );
        assert!(opcodes(&list[..list.len() - 1]).is_none());
        assert!(opcodes(&[]).unwrap().is_empty());
    }
}
