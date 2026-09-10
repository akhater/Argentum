//! Everything Argentum does to a RAW as it is decoded. One anchor.
//!
//! `raw_processing.rs` calls `on_raw_decoded` once, immediately after rawler
//! hands back the decoded image and before anything is done with it. That call
//! is the entire upstream cost of every decode-level fix we will ever make.
//!
//! The alternative — a call per fix — is one line of their file each time, in
//! the function every RAW passes through. This way the file that changes is
//! ours.
//!
//! ORDER IS THE CONTRACT
//!
//! Steps run top to bottom as written here. Anything that corrects *levels* has
//! to come before anything that reads pixel values, because the levels decide
//! what those values mean.

use rawler::rawimage::RawImage;

/// Called once per decoded RAW, before its pixels are used for anything.
///
/// `file_bytes` is the original file, because most of what has to be corrected
/// at this stage is written in the maker's own metadata rather than in anything
/// rawler exposes.
pub fn on_raw_decoded(raw: &mut RawImage, file_bytes: &[u8]) {
    // Canon sRAW / mRAW: black already subtracted, white level in the file.
    // Must be first — it decides what every pixel value means.
    super::sraw_levels::fix(raw, file_bytes);
}
