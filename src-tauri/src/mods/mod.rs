//! Argentum's own code.
//!
//! Everything in here is ours — upstream RapidRAW has never seen these files,
//! so they can't conflict when we merge its updates. Harvested algorithms get a
//! header naming the source file and upstream commit; the matching reference
//! copy lives in `docs/harvest/`.
//!
//! See `docs/ARCHITECTURE.md` for why the split matters, and
//! `docs/ADDING_A_TOOL.md` for the recipe.

pub mod auto_wb;
pub mod cache_version;
pub mod clipping;
pub mod colour_compare;
pub mod commands;
pub mod dcp;
pub mod decode;
pub mod dispatch;
pub mod display_monitor;
pub mod display_profile;
pub mod highlights;
pub mod lens_crop;
pub mod makernote_lens;
pub mod preview_encode;
pub mod profile_correction;
pub mod profile_matrix;
pub mod profiles;
pub mod profiles_online;
pub mod shader_check;
pub mod sigmoid;
pub mod sraw_levels;
pub mod startup;
