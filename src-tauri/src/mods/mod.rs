//! Argentum's own code.
//!
//! Everything in here is ours — upstream RapidRAW has never seen these files,
//! so they can't conflict when we merge its updates. Harvested algorithms get a
//! header naming the source file and upstream commit; the matching reference
//! copy lives in `docs/harvest/`.
//!
//! See `docs/ARCHITECTURE.md` for why the split matters, and
//! `docs/ADDING_A_TOOL.md` for the recipe.

#[cfg(test)]
mod ag_contract;
pub mod ag_settings;
pub mod ai_session;
pub mod atomic_write;
pub mod auto_wb;
pub mod auto_wb_photo;
pub mod black_white;
pub mod cache_key;
pub mod cache_version;
pub mod canon_makernote;
pub mod canon_old_wb;
pub mod capture_bake;
pub mod clipping;
pub mod color_grade;
pub mod colour_compare;
pub mod commands;
#[cfg(test)]
mod compat_corpus;
#[cfg(test)]
mod compat_fixture;
#[cfg(test)]
mod compat_snapshot;
pub mod copy_keys;
pub mod dcp;
pub mod decode;
pub mod dispatch;
pub mod display_monitor;
pub mod display_profile;
pub mod dng_colour;
pub mod dng_standard;
pub mod drives;
pub mod edit_status;
pub mod editor_layout;
pub mod engine_bridge;
pub mod engine_frame;
pub mod esam3;
pub mod export_metadata;
pub mod export_precision;
pub mod face_mask;
pub mod file_ops;
pub mod film_lut;
pub mod folder_counts;
pub mod gpu_runtime;
pub mod highlight_protection;
pub mod highlights;
pub mod history;
pub mod input_stage;
pub mod lens_builtin;
pub mod lens_crop;
pub mod lens_name;
pub mod lens_shot;
#[cfg(test)]
mod look_check;
pub mod look_picks;
pub mod looks;
pub mod lut_cache;
pub mod lut_provider;
pub mod makernote_lens;
pub mod mask_falloff;
pub mod memory;
pub mod model_catalog;
pub mod model_download;
pub mod model_manager;
pub mod modern_start;
pub mod preview_encode;
pub mod profile_correction;
pub mod profile_matrix;
pub mod profiles;
pub mod profiles_online;
pub mod provider_look;
pub mod raw_decoder;
pub mod raw_tone;
pub mod recent_folders;
pub mod redecode;
#[cfg(test)]
mod render_check;
pub mod restore;
pub mod salvage;
pub mod selection;
pub mod selection_models;
pub mod shader_check;
pub mod sharpen;
pub mod sharpen_gpu;
pub mod shell_photos;
pub mod snapshots;
#[cfg(test)]
mod speed_budgets;
pub mod sraw_levels;
pub mod startup;
pub mod super_resolution;
pub mod text_mask;
pub mod tif_raw;
#[cfg(test)]
mod ui_thread;
pub mod viewer_surface;
pub mod wb_legacy;
