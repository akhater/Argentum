//! Our own copy of every file Argentum downloads. Ours.
//!
//! Each file Argentum fetches, at build time or in the app, used to come from
//! one address on someone else's server: when that server went, the build or
//! the feature went with it. So each is also kept, byte for byte and under its
//! published name, as an asset of a GitHub release of `akhater/argentum-models`
//! (its release notes name every file's source and licence). That copy is
//! fetched first and the original address is the fallback; both are checked
//! against the same pinned SHA-256, so neither can hand over anything else.
//!
//! `build.rs` includes this file with `#[path]` too, so the address is written
//! here once for the build and the app.

/// Where our copies are kept: `mirror!()` is every release's parent,
/// `mirror!(release)` one release's folder, `mirror!(release, file)` one file.
macro_rules! mirror {
    () => {
        "https://github.com/akhater/argentum-models/releases/download/"
    };
    ($release:literal $(, $file:literal)?) => {
        concat!(mirror!(), $release, "/" $(, $file)?)
    };
}
pub(crate) use mirror;
