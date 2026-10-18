//! Today's app code, compiled outside the app.
//!
//! The engine's tools are copies of modules the app still runs. The parity
//! tests have to compare each copy with the original on the same input, and
//! building the app to do that would defeat the point of a crate that tests
//! on its own. So the original files are compiled here as they are, byte for
//! byte, straight out of `src-tauri/src/` - nothing is copied into this crate.
//!
//! The few things those files reach for in the rest of the app are stood in
//! for below, by name only: a GPU context with a device and a queue, settings
//! and file functions the tests never call, sRGB's primaries, and the
//! multi-exposure exception, which the engine leaves out too. When one of
//! those files changes what it imports, this crate stops compiling, which is
//! the right moment to look at whether the engine's copy has fallen behind.
//!
//! The app's shader functions are not here: the parity tests compile
//! `src-tauri/src/shaders/` themselves and run them on the GPU
//! (`tests/common/app_shader.rs`).
//!
//! Test-only. Nothing in the engine depends on it.

// Someone else's files, compiled for their behaviour and not linted here.
#![allow(warnings)]

#[path = "../../../src/mods/ag_settings.rs"]
pub mod ag_settings;
#[path = "../../../src/mods/highlights.rs"]
pub mod highlights;
#[path = "../../../src/raw_processing.rs"]
pub mod raw_processing;
#[path = "../../../src/mods/sharpen.rs"]
pub mod sharpen;
#[path = "../../../src/mods/sharpen_gpu.rs"]
pub mod sharpen_gpu;
#[path = "../../../src/white_balance.rs"]
pub mod white_balance;

/// `crate::mods`, for the files that live there and name each other by it.
pub mod mods {
    #[path = "../../../../src/mods/dcp.rs"]
    pub mod dcp;
    #[path = "../../../../src/mods/dng_standard.rs"]
    pub mod dng_standard;
    #[path = "../../../../src/mods/profiles.rs"]
    pub mod profiles;
    #[path = "../../../../src/mods/raw_tone.rs"]
    pub mod raw_tone;

    /// `mods::decode`: the decode hook and the profile's daylight matrix,
    /// neither of which the tests reach.
    pub mod decode {
        use rawler::rawimage::RawImage;

        pub type Matrices = (u16, [f32; 9], Option<[f32; 9]>);

        pub fn daylight_matrix(_profile: &super::dcp::Profile) -> Option<Matrices> {
            None
        }

        pub fn on_raw_decoded(_raw: &mut RawImage, _file_bytes: &[u8], _photo_path: Option<&str>) {}
    }
}

/// `crate::image_processing`: the GPU context `sharpen_gpu.rs` reads, and
/// sRGB's primaries and white for `white_balance.rs`, as
/// `src-tauri/src/image_processing.rs` defines them.
pub mod image_processing {
    use std::sync::Arc;

    use glam::{Mat3, Vec2, Vec3};
    use image::DynamicImage;
    use rawler::decoders::Orientation;

    pub struct GpuContext {
        pub device: Arc<wgpu::Device>,
        pub queue: Arc<wgpu::Queue>,
    }

    pub(crate) const WP_D65: Vec2 = Vec2::new(0.3127, 0.3290);
    pub(crate) const PRIMARIES_SRGB: [Vec2; 3] = [
        Vec2::new(0.64, 0.33),
        Vec2::new(0.30, 0.60),
        Vec2::new(0.15, 0.06),
    ];

    fn xy_to_xyz(xy: Vec2) -> Vec3 {
        if xy.y < 1e-6 {
            Vec3::ZERO
        } else {
            Vec3::new(xy.x / xy.y, 1.0, (1.0 - xy.x - xy.y) / xy.y)
        }
    }

    pub(crate) fn primaries_to_xyz_matrix(primaries: &[Vec2; 3], white_point: Vec2) -> Mat3 {
        let r_xyz = xy_to_xyz(primaries[0]);
        let g_xyz = xy_to_xyz(primaries[1]);
        let b_xyz = xy_to_xyz(primaries[2]);
        let primaries_matrix = Mat3::from_cols(r_xyz, g_xyz, b_xyz);
        let white_point_xyz = xy_to_xyz(white_point);
        let s = primaries_matrix.inverse() * white_point_xyz;
        Mat3::from_cols(r_xyz * s.x, g_xyz * s.y, b_xyz * s.z)
    }

    /// Never reached: the tests develop nothing through the app.
    pub fn apply_orientation(image: DynamicImage, _orientation: Orientation) -> DynamicImage {
        image
    }

    /// Never reached: `raw_tone`'s matching against the camera's JPEG.
    pub fn downscale_f32_image(image: &DynamicImage, _nwidth: u32, _nheight: u32) -> DynamicImage {
        image.clone()
    }
}

/// `crate::multi_exposure`: the as-shot white balance's exception for
/// in-camera multiple exposures, left out here as the engine leaves it out.
pub mod multi_exposure {
    pub fn neutralize_wb_if_multiexposure(wb_coeffs: [f32; 4], _file_bytes: &[u8]) -> [f32; 4] {
        wb_coeffs
    }
}

/// `crate::file_management`, as the app's files name it. The tests hand
/// those files bytes and decoded images, never a path.
pub mod file_management {
    use std::path::{Path, PathBuf};

    pub fn parse_virtual_path(virtual_path: &str) -> (PathBuf, PathBuf) {
        (PathBuf::from(virtual_path), PathBuf::new())
    }

    pub fn read_file_mapped(_path: &Path) -> Result<Vec<u8>, String> {
        Err("the reference build reads no files".into())
    }
}

/// `crate::formats`, never reached.
pub mod formats {
    pub fn is_raw_file<P: AsRef<std::path::Path>>(_path: P) -> bool {
        false
    }
}

/// `crate::image_loader`, never reached.
pub mod image_loader {
    pub fn embedded_preview_fallback(_bytes: &[u8]) -> Option<image::DynamicImage> {
        None
    }
}

/// `crate::app_settings`, as `sharpen::retire_presharpening_once` and
/// `raw_tone`'s JPEG matching name it. Both need the running app and are
/// never called here.
pub mod app_settings {
    pub struct AppSettings {
        pub raw_preprocessing_sharpening: Option<f32>,
        pub raw_highlight_compression: Option<f32>,
        pub linear_raw_mode: String,
    }

    pub fn load_settings(_app: tauri::AppHandle) -> Result<AppSettings, String> {
        Err("the app's settings are not part of the reference build".into())
    }

    pub fn save_settings(_settings: AppSettings, _app: tauri::AppHandle) -> Result<(), String> {
        Err("the app's settings are not part of the reference build".into())
    }
}
