//! White balance as a temperature in kelvin and a tint, starting at the
//! camera's own: RapidRAW 1.6.5's, as the app runs it.
//!
//! The develop has already balanced the picture for the light the camera
//! measured (rawler's as-shot multipliers and colour matrix). This tool reads
//! that light as a kelvin and tint, and when the settings name another one
//! adapts the picture from the one to the other: Bradford cone gains, applied
//! in LMS and normalised so white keeps its luminance.
//!
//! COPIED FROM THE APP @ c786c146
//!
//! - `WhiteBalance`, the Robertson isotherms, `adaptation_log_gains` and
//!   `rgb_to_lms`: `src-tauri/src/white_balance.rs`, RapidRAW 1.6.5's Kelvin
//!   engine as Argentum adopted it (DEC-20).
//! - The as-shot light: `read_as_shot_white_balance` in
//!   `src-tauri/src/raw_processing.rs`, reading the decoded `RawImage` the
//!   engine already holds rather than the file again.
//! - sRGB's primaries and white: `src-tauri/src/image_processing.rs`.
//! - The per-pixel step: `apply_white_balance` in
//!   `src-tauri/src/shaders/shader.wgsl`, run in the pixel pass, its two
//!   matrices handed in as parameters rather than read from RapidRAW's
//!   uniform.
//!
//! Left out: the relative sliders (`shifted`), the picker and the
//! multi-exposure exception (`neutralize_wb_if_multiexposure`), which the
//! engine's develop leaves out too.

use std::sync::Mutex;

use glam::{DMat3, DVec3, Mat3, Vec2, Vec3};
use rawler::imgop::xyz::Illuminant;
use rawler::rawimage::{RawImage, RawPhotometricInterpretation};
use serde::{Deserialize, Serialize};

use crate::{Context, Field, Pixel, Section, Source, Stage, Tool};

pub const MIN_TEMPERATURE_K: f64 = 2000.0;
pub const MAX_TEMPERATURE_K: f64 = 50000.0;
pub const MAX_TINT: f64 = 150.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// The colour temperature of the light, on the Planckian locus.
    pub temperature_k: f64,
    /// Off the locus: positive is magenta, negative green, in the DNG SDK's
    /// tint units.
    pub tint: f64,
}

/// D65, as the app takes a photo whose light it cannot read. A photo's own
/// starting point is the camera's (`Tool::defaults_for`).
impl Default for Settings {
    fn default() -> Self {
        WhiteBalance::reference().into()
    }
}

impl crate::Settings for Settings {
    const VERSION: u32 = 1;
    const FIELDS: &'static [Field] = &[
        Field::new("temperature_k")
            .range(MIN_TEMPERATURE_K, MAX_TEMPERATURE_K)
            .step(10.0)
            .unit("K")
            .log(),
        Field::new("tint").range(-MAX_TINT, MAX_TINT).step(1.0),
    ];
}

impl From<WhiteBalance> for Settings {
    fn from(wb: WhiteBalance) -> Self {
        Self {
            temperature_k: wb.temperature,
            tint: wb.tint,
        }
    }
}

#[derive(Default)]
pub struct RrWhiteBalance {
    /// The last photo's as-shot light, by source key.
    as_shot: Mutex<Option<(u64, WhiteBalance)>>,
}

impl Tool for RrWhiteBalance {
    const ID: &'static str = "rr_white_balance";
    const STAGE: Stage = Stage::SceneLinear;
    const SECTION: Section = Section::Basic;
    const PIXEL: Option<Pixel> = Some(Pixel {
        wgsl: WGSL,
        params: 7,
        detail: false,
    });
    type Settings = Settings;

    fn defaults_for(&self, source: &Source) -> Settings {
        self.as_shot(source).into()
    }

    /// RGB to LMS and back, column by column, as the app hands them to its
    /// shader, then the cone gains from the light the camera measured to the
    /// one set.
    fn pixel_params(&self, settings: &Settings, cx: &Context) -> Option<Vec<[f32; 4]>> {
        let target = WhiteBalance {
            temperature: settings.temperature_k,
            tint: settings.tint,
        };
        let [l, m, s] = adaptation_log_gains(self.as_shot(cx.source), target);
        // As the shader: no gains, no change.
        if [l, m, s] == [0.0; 3] {
            return None;
        }
        let to_lms = rgb_to_lms();
        let from_lms = to_lms.inverse();
        let columns = |m: Mat3| [0, 1, 2].map(|i| m.col(i).extend(0.0).to_array());
        let mut params = columns(to_lms).to_vec();
        params.extend(columns(from_lms));
        params.push([l, m, s, 0.0]);
        Some(params)
    }
}

impl RrWhiteBalance {
    fn as_shot(&self, source: &Source) -> WhiteBalance {
        let mut memo = self.as_shot.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((key, wb)) = *memo
            && key == source.key
        {
            return wb;
        }
        let wb = as_shot(&source.raw).unwrap_or_else(WhiteBalance::reference);
        *memo = Some((source.key, wb));
        wb
    }
}

/// The light the camera measured, from the decoded image: its as-shot
/// multipliers through its colour matrices. `None` where the app reads none.
// Copied from src-tauri/src/raw_processing.rs @ c786c146,
// `read_as_shot_white_balance`, given the decoded image instead of the file.
pub fn as_shot(raw_image: &RawImage) -> Option<WhiteBalance> {
    if raw_image.cpp == 1 && !matches!(raw_image.photometric, RawPhotometricInterpretation::Cfa(_))
    {
        return None;
    }

    let wb_coeffs = raw_image.wb_coeffs;
    let neutral = if wb_coeffs[0].is_nan() {
        [1.0; 4]
    } else {
        wb_coeffs.map(|c| 1.0 / c)
    };

    let matrices = &raw_image.color_matrix;
    if let (Some(matrix_a), Some(matrix_d65)) =
        (matrices.get(&Illuminant::A), matrices.get(&Illuminant::D65))
    {
        return WhiteBalance::from_dual_illuminant_camera_neutral(matrix_a, matrix_d65, &neutral);
    }

    let color_matrix = matrices
        .get(&Illuminant::D65)
        .or_else(|| matrices.values().next())?;
    WhiteBalance::from_camera_neutral(color_matrix, &neutral)
}

// ---------------------------------------------------------------------------
// Copied from src-tauri/src/white_balance.rs @ c786c146 (RapidRAW 1.6.5).
// ---------------------------------------------------------------------------

const TINT_SCALE: f64 = -3000.0;
const ILLUMINANT_A_TEMPERATURE: f64 = 2856.0;
const ILLUMINANT_D65_TEMPERATURE: f64 = 6504.0;
const MIN_LMS_FRACTION_OF_LOCUS: f64 = 0.25;

const BRADFORD: DMat3 = DMat3::from_cols_array(&[
    0.8951, -0.7502, 0.0389, 0.2664, 1.7135, -0.0685, -0.1614, 0.0367, 1.0296,
]);

const ROBERTSON_ISOTHERMS: [[f64; 4]; 31] = [
    [0.0, 0.18006, 0.26352, -0.24341],
    [10.0, 0.18066, 0.26589, -0.25479],
    [20.0, 0.18133, 0.26846, -0.26876],
    [30.0, 0.18208, 0.27119, -0.28539],
    [40.0, 0.18293, 0.27407, -0.30470],
    [50.0, 0.18388, 0.27709, -0.32675],
    [60.0, 0.18494, 0.28021, -0.35156],
    [70.0, 0.18611, 0.28342, -0.37915],
    [80.0, 0.18740, 0.28668, -0.40955],
    [90.0, 0.18880, 0.28997, -0.44278],
    [100.0, 0.19032, 0.29326, -0.47888],
    [125.0, 0.19462, 0.30141, -0.58204],
    [150.0, 0.19962, 0.30921, -0.70471],
    [175.0, 0.20525, 0.31647, -0.84901],
    [200.0, 0.21142, 0.32312, -1.0182],
    [225.0, 0.21807, 0.32909, -1.2168],
    [250.0, 0.22511, 0.33439, -1.4512],
    [275.0, 0.23247, 0.33904, -1.7298],
    [300.0, 0.24010, 0.34308, -2.0637],
    [325.0, 0.24792, 0.34655, -2.4681],
    [350.0, 0.25591, 0.34951, -2.9641],
    [375.0, 0.26400, 0.35200, -3.5814],
    [400.0, 0.27218, 0.35407, -4.3633],
    [425.0, 0.28039, 0.35577, -5.3762],
    [450.0, 0.28863, 0.35714, -6.7262],
    [475.0, 0.29685, 0.35823, -8.5955],
    [500.0, 0.30505, 0.35907, -11.324],
    [525.0, 0.31320, 0.35968, -15.628],
    [550.0, 0.32129, 0.36011, -23.325],
    [575.0, 0.32931, 0.36038, -40.770],
    [600.0, 0.33724, 0.36051, -116.45],
];

/// A light, as a kelvin and a tint.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WhiteBalance {
    pub temperature: f64,
    pub tint: f64,
}

fn isotherm_direction(slope: f64) -> (f64, f64) {
    let len = (1.0 + slope * slope).sqrt();
    (1.0 / len, slope / len)
}

impl WhiteBalance {
    pub fn reference() -> Self {
        Self::from_xy(WP_D65.x as f64, WP_D65.y as f64)
    }

    fn from_xyz(xyz: DVec3) -> Option<Self> {
        let sum = xyz.element_sum();
        if !sum.is_finite() || sum <= 0.0 || xyz.y <= 0.0 {
            return None;
        }
        Some(Self::from_xy(xyz.x / sum, xyz.y / sum))
    }

    pub fn from_xy(x: f64, y: f64) -> Self {
        let denom = 1.5 - x + 6.0 * y;
        let u = 2.0 * x / denom;
        let v = 3.0 * y / denom;

        let mut last_distance = 0.0;
        let mut last_direction = (0.0, 0.0);
        for i in 1..ROBERTSON_ISOTHERMS.len() {
            let [mired, iso_u, iso_v, slope] = ROBERTSON_ISOTHERMS[i];
            let direction = isotherm_direction(slope);
            let distance = (v - iso_v) * direction.0 - (u - iso_u) * direction.1;
            let is_last = i == ROBERTSON_ISOTHERMS.len() - 1;

            if distance <= 0.0 || is_last {
                let distance = (-distance).max(0.0);
                let f = if i == 1 {
                    0.0
                } else {
                    distance / (last_distance + distance)
                };
                let [prev_mired, prev_u, prev_v, _] = ROBERTSON_ISOTHERMS[i - 1];
                let locus_u = prev_u * f + iso_u * (1.0 - f);
                let locus_v = prev_v * f + iso_v * (1.0 - f);
                let du = direction.0 * (1.0 - f) + last_direction.0 * f;
                let dv = direction.1 * (1.0 - f) + last_direction.1 * f;
                let len = (du * du + dv * dv).sqrt();
                let offset = ((u - locus_u) * du + (v - locus_v) * dv) / len;

                return Self {
                    temperature: 1.0e6 / (prev_mired * f + mired * (1.0 - f)),
                    tint: offset * TINT_SCALE,
                };
            }

            last_distance = distance;
            last_direction = direction;
        }
        unreachable!()
    }

    pub fn to_xy(self) -> (f64, f64) {
        let mired = 1.0e6 / self.temperature;
        let i = ROBERTSON_ISOTHERMS
            .iter()
            .skip(1)
            .position(|row| mired < row[0])
            .map_or(ROBERTSON_ISOTHERMS.len() - 1, |p| p + 1);

        let [prev_mired, prev_u, prev_v, prev_slope] = ROBERTSON_ISOTHERMS[i - 1];
        let [next_mired, next_u, next_v, next_slope] = ROBERTSON_ISOTHERMS[i];
        let f = (next_mired - mired) / (next_mired - prev_mired);

        let prev_direction = isotherm_direction(prev_slope);
        let next_direction = isotherm_direction(next_slope);
        let du = prev_direction.0 * f + next_direction.0 * (1.0 - f);
        let dv = prev_direction.1 * f + next_direction.1 * (1.0 - f);
        let len = (du * du + dv * dv).sqrt();

        let u = prev_u * f + next_u * (1.0 - f) + du / len * self.tint / TINT_SCALE;
        let v = prev_v * f + next_v * (1.0 - f) + dv / len * self.tint / TINT_SCALE;
        let denom = u - 4.0 * v + 2.0;
        (1.5 * u / denom, v / denom)
    }

    fn chromaticity_lms(self) -> DVec3 {
        let (x, y) = self.to_xy();
        BRADFORD * DVec3::new(x / y, 1.0, (1.0 - x - y) / y)
    }

    fn lms(self) -> DVec3 {
        let locus = Self { tint: 0.0, ..self }.chromaticity_lms();
        self.chromaticity_lms()
            .max(locus * MIN_LMS_FRACTION_OF_LOCUS)
    }

    pub fn from_camera_neutral(xyz_to_camera: &[f32], neutral: &[f32]) -> Option<Self> {
        let channels = xyz_to_camera.len() / 3;
        if channels < 3 || neutral.len() < channels {
            return None;
        }
        let (normal, rhs) = xyz_to_camera.as_chunks::<3>().0.iter().zip(neutral).fold(
            (DMat3::ZERO, DVec3::ZERO),
            |(normal, rhs), (row, &n)| {
                let r = DVec3::from_array(row.map(f64::from));
                (
                    normal + DMat3::from_cols(r * r.x, r * r.y, r * r.z),
                    rhs + r * n as f64,
                )
            },
        );
        if normal.determinant().abs() < 1e-12 {
            return None;
        }
        Self::from_xyz(normal.inverse() * rhs)
    }

    pub fn from_dual_illuminant_camera_neutral(
        xyz_to_camera_a: &[f32],
        xyz_to_camera_d65: &[f32],
        neutral: &[f32],
    ) -> Option<Self> {
        if xyz_to_camera_a.len() != xyz_to_camera_d65.len() {
            return Self::from_camera_neutral(xyz_to_camera_d65, neutral);
        }

        let mut estimate = Self::from_camera_neutral(xyz_to_camera_d65, neutral)?;
        for _ in 0..20 {
            let weight = ((1.0 / estimate.temperature - 1.0 / ILLUMINANT_D65_TEMPERATURE)
                / (1.0 / ILLUMINANT_A_TEMPERATURE - 1.0 / ILLUMINANT_D65_TEMPERATURE))
                .clamp(0.0, 1.0) as f32;
            let xyz_to_camera: Vec<f32> = xyz_to_camera_a
                .iter()
                .zip(xyz_to_camera_d65)
                .map(|(a, d65)| weight * a + (1.0 - weight) * d65)
                .collect();
            let next = Self::from_camera_neutral(&xyz_to_camera, neutral)?;
            let converged = (next.temperature - estimate.temperature).abs() < 0.1;
            estimate = next;
            if converged {
                break;
            }
        }
        Some(estimate)
    }
}

pub fn adaptation_log_gains(current: WhiteBalance, target: WhiteBalance) -> [f32; 3] {
    (current.lms() / target.lms())
        .to_array()
        .map(|gain| gain.ln() as f32)
}

pub fn rgb_to_lms() -> Mat3 {
    (BRADFORD * primaries_to_xyz_matrix(&PRIMARIES_SRGB, WP_D65).as_dmat3()).as_mat3()
}

// From src-tauri/src/image_processing.rs @ c786c146.

const WP_D65: Vec2 = Vec2::new(0.3127, 0.3290);
const PRIMARIES_SRGB: [Vec2; 3] = [
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

fn primaries_to_xyz_matrix(primaries: &[Vec2; 3], white_point: Vec2) -> Mat3 {
    let r_xyz = xy_to_xyz(primaries[0]);
    let g_xyz = xy_to_xyz(primaries[1]);
    let b_xyz = xy_to_xyz(primaries[2]);
    let primaries_matrix = Mat3::from_cols(r_xyz, g_xyz, b_xyz);
    let white_point_xyz = xy_to_xyz(white_point);
    let s = primaries_matrix.inverse() * white_point_xyz;
    Mat3::from_cols(r_xyz * s.x, g_xyz * s.y, b_xyz * s.z)
}

const WGSL: &str = r#"
// apply_white_balance, src-tauri/src/shaders/shader.wgsl @ c786c146
// (RapidRAW 1.6.5), with its two matrices handed in rather than read from
// RapidRAW's uniform.
fn apply_white_balance(
    color: vec3<f32>,
    log_gains: vec3<f32>,
    rgb_to_lms: mat3x3<f32>,
    lms_to_rgb: mat3x3<f32>,
) -> vec3<f32> {
    if (all(log_gains == vec3<f32>(0.0))) {
        return color;
    }
    let gains = exp(log_gains);
    let white = lms_to_rgb * (gains * (rgb_to_lms * vec3<f32>(1.0)));
    return lms_to_rgb * (gains * (rgb_to_lms * color)) / get_luma(white);
}

fn rr_white_balance(c: vec3<f32>, px: Px) -> vec3<f32> {
    let p = px.p;
    let rgb_to_lms = mat3x3<f32>(ag_params[p].xyz, ag_params[p + 1u].xyz, ag_params[p + 2u].xyz);
    let lms_to_rgb = mat3x3<f32>(ag_params[p + 3u].xyz, ag_params[p + 4u].xyz, ag_params[p + 5u].xyz);
    return apply_white_balance(c, ag_params[p + 6u].xyz, rgb_to_lms, lms_to_rgb);
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_light_is_no_change() {
        let wb = WhiteBalance {
            temperature: 5200.0,
            tint: 7.0,
        };
        assert_eq!(adaptation_log_gains(wb, wb), [0.0; 3]);
    }

    #[test]
    fn kelvin_and_tint_go_there_and_back() {
        for (t, tint) in [(2856.0, 0.0), (5003.0, 10.0), (6504.0, -20.0), (12000.0, 40.0)] {
            let (x, y) = WhiteBalance { temperature: t, tint }.to_xy();
            let back = WhiteBalance::from_xy(x, y);
            assert!((back.temperature - t).abs() < 0.5, "{t}: {back:?}");
            assert!((back.tint - tint).abs() < 0.05, "{tint}: {back:?}");
        }
    }
}
