pub fn is_incamera_multiexposure_canon(file_bytes: &[u8]) -> bool {
    if file_bytes.len() < 8 {
        return false;
    }

    match file_bytes.get(0..4) {
        Some([0x49, 0x49, 0x2A, 0x00]) => {}
        _ => return false,
    }

    let walk = || -> Option<bool> {
        let b: [u8; 4] = file_bytes.get(4..8)?.try_into().ok()?;
        let ifd0_offset = u32::from_le_bytes(b) as usize;
        let exif_ifd_offset = _find_ifd_entry(file_bytes, ifd0_offset, 0x8769)? as usize;
        let maker_note_offset = _find_ifd_entry(file_bytes, exif_ifd_offset, 0x927C)? as usize;
        let multi_exp_block_offset =
            _find_ifd_entry(file_bytes, maker_note_offset, 0x4021)? as usize;
        let flag_offset = multi_exp_block_offset + 4;
        let v: [u8; 4] = file_bytes
            .get(flag_offset..flag_offset + 4)?
            .try_into()
            .ok()?;
        Some(u32::from_le_bytes(v) == 1)
    };

    walk().unwrap_or(false)
}

fn _find_ifd_entry(file_bytes: &[u8], ifd_offset: usize, tag_id: u16) -> Option<u32> {
    let rd16 = |offset: usize| -> Option<u16> {
        let b: [u8; 2] = file_bytes.get(offset..offset + 2)?.try_into().ok()?;
        Some(u16::from_le_bytes(b))
    };

    let rd32 = |offset: usize| -> Option<u32> {
        let b: [u8; 4] = file_bytes.get(offset..offset + 4)?.try_into().ok()?;
        Some(u32::from_le_bytes(b))
    };

    let entry_count = rd16(ifd_offset)? as usize;
    let capped_count = entry_count.min(512);

    for i in 0..capped_count {
        let entry_offset = ifd_offset + 2 + i * 12;
        let tag = rd16(entry_offset)?;

        if tag == tag_id {
            return rd32(entry_offset + 8);
        }
    }

    None
}

pub fn neutralize_wb_if_multiexposure(wb_coeffs: [f32; 4], file_bytes: &[u8]) -> [f32; 4] {
    if is_incamera_multiexposure_canon(file_bytes) {
        log::info!("[raw_hdr_wb] multi-exposure CR2 detected, neutralizing WB");
        let mut neutralized = wb_coeffs;
        for exp in &mut neutralized {
            if exp.is_finite() {
                *exp = 1.0;
            }
        }
        neutralized
    } else {
        wb_coeffs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One little-endian IFD entry of a single LONG value.
    fn long_entry(tag: u16, value: u32) -> [u8; 12] {
        let mut e = [0u8; 12];
        e[0..2].copy_from_slice(&tag.to_le_bytes());
        e[2..4].copy_from_slice(&4u16.to_le_bytes());
        e[4..8].copy_from_slice(&1u32.to_le_bytes());
        e[8..12].copy_from_slice(&value.to_le_bytes());
        e
    }

    /// A CR2 whose in-camera multi-exposure flag is set, laid out as the walk reads it.
    fn multi_exposure_cr2() -> Vec<u8> {
        let mut f = b"II*\0".to_vec();
        f.extend(8u32.to_le_bytes()); // IFD0 at 8
        f.extend(1u16.to_le_bytes());
        f.extend(long_entry(0x8769, 26)); // Exif IFD at 26
        f.extend(0u32.to_le_bytes());
        f.extend(1u16.to_le_bytes());
        f.extend(long_entry(0x927C, 44)); // maker note at 44
        f.extend(0u32.to_le_bytes());
        f.extend(1u16.to_le_bytes());
        f.extend(long_entry(0x4021, 62)); // block at 62
        f.extend(0u32.to_le_bytes());
        f.extend(0u32.to_le_bytes()); // block: version 0
        f.extend(1u32.to_le_bytes()); // flag 1 at 66
        f
    }

    #[test]
    fn a_multi_exposure_flag_is_read() {
        assert!(is_incamera_multiexposure_canon(&multi_exposure_cr2()));
    }

    /// A file shorter than its own header is not multi-exposure: no assertion, no panic.
    #[test]
    fn a_file_shorter_than_the_header_is_not_multi_exposure() {
        assert!(!is_incamera_multiexposure_canon(&[0x49, 0x49, 0x2A, 0x00]));
    }
}
