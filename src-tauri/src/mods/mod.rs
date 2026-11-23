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
pub mod apex_aperture;
pub mod app_stamp;
pub mod atomic_write;
pub mod auto_wb;
pub mod auto_wb_photo;
pub mod black_white;
// build.rs includes it; here only so its tests run.
#[cfg(test)]
#[allow(dead_code)]
mod build_ort;
pub mod cache_key;
pub mod cache_version;
pub mod camera_look;
pub mod canon_crop_mode;
pub mod canon_makernote;
pub mod canon_old_wb;
pub mod capture_bake;
pub mod clear_scope;
pub mod clipping;
pub mod clone_heal;
pub mod close_failsafe;
pub mod color_grade;
#[cfg(test)]
mod colour_compare;
pub mod commands;
#[cfg(test)]
mod compat_corpus;
#[cfg(test)]
mod compat_fixture;
#[cfg(test)]
mod compat_snapshot;
pub mod copy_keys;
pub mod crash_log;
pub mod dcp;
pub mod decode;
pub mod decode_limits;
#[cfg(test)]
mod demosaic_measure;
pub mod denoise_gpu;
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
pub mod events;
pub mod export_at_size;
pub mod export_batch;
pub mod export_metadata;
pub mod export_precision;
pub mod face_mask;
pub mod file_ops;
pub mod film_lut;
pub mod move_files;
pub mod focus_points;
pub mod folder_counts;
pub mod folder_walk;
pub mod generated_image;
pub mod gpu_backend_fallback;
pub mod gpu_pipelines;
pub mod gpu_runtime;
pub mod gpu_setup_mark;
pub mod gpu_size_limit;
#[cfg(test)]
mod gpu_tests;
pub mod headless_gpu;
pub mod heic;
pub mod highlight_protection;
pub mod highlight_pull;
#[cfg(test)]
mod highlight_pull_measure;
pub mod highlight_rule;
pub mod highlights;
pub mod history;
pub mod icc_input;
#[cfg(test)]
mod icc_input_measure;
pub mod input_stage;
pub mod jpeg_exif_fit;
pub mod jxl_export;
pub mod lens_builtin;
pub mod lens_crop;
pub mod lens_db;
pub mod lens_flange;
pub mod lens_focal_span;
pub mod lens_frame;
pub mod lens_interp;
pub mod lens_match;
pub mod lens_name;
pub mod lens_names;
mod lens_reference;
pub mod lens_shot;
pub mod lens_units;
#[cfg(test)]
mod look_check;
pub mod look_grain;
pub mod look_picks;
pub mod looks;
pub mod lr_legacy;
pub mod lut_cache;
pub mod lut_provider;
pub mod makernote_lens;
pub mod mask_falloff;
pub mod mask_feather;
#[cfg(test)]
pub mod mem_probe;
pub mod memory;
pub mod mirror;
pub mod model_catalog;
pub mod model_download;
pub mod model_manager;
pub mod modern_start;
pub mod photo_frame;
pub mod presets_read;
pub mod preview_encode;
pub mod preview_jpeg;
pub mod process_tree;
pub mod profile_correction;
pub mod profile_matrix;
pub mod profiles;
pub mod profiles_online;
pub mod provider_look;
pub mod raw_decoder;
pub mod raw_tone;
pub mod recent_folders;
pub mod recycle;
pub mod redecode;
#[cfg(test)]
mod render_check;
pub mod restore;
pub mod retired_keys;
pub mod rr_demosaic;
pub mod rt_profiles;
pub mod salvage;
pub mod selection;
pub mod selection_again;
pub mod selection_models;
pub mod shader_check;
pub mod sharpen;
pub mod sharpen_gpu;
pub mod shell_photos;
pub mod sidecar_read;
pub mod snapshots;
#[cfg(test)]
mod speed_budgets;
pub mod sraw_levels;
pub mod startup;
pub mod startup_marks;
pub mod super_resolution;
pub mod text_mask;
pub mod tif_raw;
pub mod touch_up;
#[cfg(test)]
mod ui_thread;
pub mod viewer_surface;
pub mod warped_photo;
pub mod wb_legacy;
pub mod worker_thread;
pub mod write_locks;
pub mod xmp_packet;
