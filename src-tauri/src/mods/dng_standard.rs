//! DNG Standard: the picture a DNG renderer makes by default, as a RAW tone
//! rendering you can choose. Ours.
//!
//! WHY
//!
//! Argentum's default RAW rendering is RapidRAW's: the sRGB curve, a 1.1 gamma
//! lift and a gentle S, in their raw branch of `shader.wgsl`. Next to Lightroom,
//! or the camera's own JPEG, it looks flat - and the decoder is not why. A DNG
//! renderer does two things to every photo that Argentum never did:
//!
//! 1. **A per-camera exposure offset**, the BaselineExposure Camera Raw gives
//!    each body before anything else. A DNG carries its own; for other files it
//!    comes from a table. The median is +0.3 EV, the range -0.5 to +0.85.
//! 2. **Adobe's default tone curve**, the "ACR3" curve of the DNG SDK. Middle
//!    grey (0.18) comes out at 0.39 - about a stop brighter - with 1.36 times
//!    the contrast at that point and the highlights rolled off.
//!
//! RAWmakase, an MIT-licensed editor that measures itself against Camera Raw,
//! pointed at both; its per-camera table is where the offsets come from.
//!
//! OPT-IN
//!
//! Nothing changes unless DNG Standard is picked under RAW > Rendering. The
//! default stays RapidRAW's.
//!
//! HOW IT REACHES THE SCREEN
//!
//! The same way as Base Curve and Auto-Matched: a 16-point curve made for the
//! photo, kept in its edit as `rawToneCurve`, handed to the GPU in their
//! `GlobalAdjustments`. The difference is how it is applied (`rt_rgb_tone` in
//! `modules.wgsl`): in linear ProPhoto RGB, to each pixel's largest and smallest
//! channel, with the middle one kept in its place between them - Adobe's
//! RGBTone. A colour keeps its hue as the curve brightens it. Applied to each
//! channel on its own, as Base Curve is, oranges drift and saturated colours
//! climb.
//!
//! The curve carries the exposure offset too, so a photo's whole default look
//! travels in its edit, and a later change to the table cannot move an edit
//! someone has already made.
//!
//! WHAT IT IS NOT
//!
//! Adobe's colour. The camera matrix is still rawler's, or the camera profile
//! chosen in Color, and no profile look table is applied. This is the *tone* of
//! a default DNG render, not its hue twists.

use std::sync::OnceLock;

use crate::mods::profiles::same_model;
use crate::mods::raw_tone::ToneCurvePoint;

/// The `rawToneRendering` value that selects this.
pub const MODE: &str = "dngStandard";

/// The photo's curve: its exposure offset and Adobe's default tone curve, as
/// the 16 points the shader takes.
pub fn curve(bytes: &[u8]) -> Result<Vec<ToneCurvePoint>, String> {
    let facts = facts(bytes)?;
    let baseline = baseline(&facts);
    log::info!(
        "DNG Standard: {} {} at ISO {:?}, exposure offset {:+.2} EV ({:?})",
        facts.make,
        facts.model,
        facts.iso,
        baseline.ev,
        baseline.origin
    );
    Ok(curve_points(baseline.ev))
}

/// The curve for a photo the renderer is handed without one: a photo nobody has
/// edited, which starts in DNG Standard (`raw_tone::mode_of`). Read from the
/// file's metadata once per photo and kept, since every render of the photo asks
/// again. Without a readable photo, the offset is the table's median.
pub fn curve_for_photo(photo: Option<&str>) -> Vec<ToneCurvePoint> {
    use std::collections::HashMap;
    use std::sync::Mutex;
    static KEPT: OnceLock<Mutex<HashMap<String, Vec<ToneCurvePoint>>>> = OnceLock::new();
    let Some(photo) = photo else {
        return curve_points(baseline(&Facts::default()).ev);
    };
    let kept = KEPT.get_or_init(Default::default);
    if let Some(points) = kept.lock().unwrap_or_else(|e| e.into_inner()).get(photo) {
        return points.clone();
    }
    let (source, _) = crate::file_management::parse_virtual_path(photo);
    let read = crate::file_management::read_file_mapped(&source)
        .map_err(|e| e.to_string())
        .and_then(|bytes| curve(&bytes));
    match read {
        Ok(points) => {
            kept.lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(photo.to_string(), points.clone());
            points
        }
        // Not kept: a file that is busy now is read again on the next render.
        Err(e) => {
            log::warn!("DNG Standard: {e}; using the median exposure offset for now");
            curve_points(baseline(&Facts::default()).ev)
        }
    }
}

// ---------------------------------------------------------------------------
// The tone curve
// ---------------------------------------------------------------------------

/// Adobe's default tone curve: 1025 samples of linear light in [0, 1], linear
/// light out, read by straight-line interpolation as the DNG SDK reads it.
///
/// From RawTherapee `rtengine/dcp.cc` @ ca9234b1, `adobe_camera_raw_default_curve`,
/// which carries it unchanged from the Adobe DNG SDK (`dng_render.cpp`, the
/// ACR3 default tone curve). Copyright Adobe Inc., distributed under the DNG SDK
/// licence: docs/licenses/Adobe-DNG-SDK.txt.
// Measured samples: 0.70711 is the curve at one point, not 1/sqrt(2).
#[rustfmt::skip]
#[allow(clippy::approx_constant)]
const ADOBE_CAMERA_RAW_DEFAULT_CURVE: [f32; 1025] = [
    0.00000, 0.00078, 0.00160, 0.00242, 0.00314, 0.00385, 0.00460, 0.00539,
    0.00623, 0.00712, 0.00806, 0.00906, 0.01012, 0.01122, 0.01238, 0.01359,
    0.01485, 0.01616, 0.01751, 0.01890, 0.02033, 0.02180, 0.02331, 0.02485,
    0.02643, 0.02804, 0.02967, 0.03134, 0.03303, 0.03475, 0.03648, 0.03824,
    0.04002, 0.04181, 0.04362, 0.04545, 0.04730, 0.04916, 0.05103, 0.05292,
    0.05483, 0.05675, 0.05868, 0.06063, 0.06259, 0.06457, 0.06655, 0.06856,
    0.07057, 0.07259, 0.07463, 0.07668, 0.07874, 0.08081, 0.08290, 0.08499,
    0.08710, 0.08921, 0.09134, 0.09348, 0.09563, 0.09779, 0.09996, 0.10214,
    0.10433, 0.10652, 0.10873, 0.11095, 0.11318, 0.11541, 0.11766, 0.11991,
    0.12218, 0.12445, 0.12673, 0.12902, 0.13132, 0.13363, 0.13595, 0.13827,
    0.14061, 0.14295, 0.14530, 0.14765, 0.15002, 0.15239, 0.15477, 0.15716,
    0.15956, 0.16197, 0.16438, 0.16680, 0.16923, 0.17166, 0.17410, 0.17655,
    0.17901, 0.18148, 0.18395, 0.18643, 0.18891, 0.19141, 0.19391, 0.19641,
    0.19893, 0.20145, 0.20398, 0.20651, 0.20905, 0.21160, 0.21416, 0.21672,
    0.21929, 0.22185, 0.22440, 0.22696, 0.22950, 0.23204, 0.23458, 0.23711,
    0.23963, 0.24215, 0.24466, 0.24717, 0.24967, 0.25216, 0.25465, 0.25713,
    0.25961, 0.26208, 0.26454, 0.26700, 0.26945, 0.27189, 0.27433, 0.27676,
    0.27918, 0.28160, 0.28401, 0.28641, 0.28881, 0.29120, 0.29358, 0.29596,
    0.29833, 0.30069, 0.30305, 0.30540, 0.30774, 0.31008, 0.31241, 0.31473,
    0.31704, 0.31935, 0.32165, 0.32395, 0.32623, 0.32851, 0.33079, 0.33305,
    0.33531, 0.33756, 0.33981, 0.34205, 0.34428, 0.34650, 0.34872, 0.35093,
    0.35313, 0.35532, 0.35751, 0.35969, 0.36187, 0.36404, 0.36620, 0.36835,
    0.37050, 0.37264, 0.37477, 0.37689, 0.37901, 0.38112, 0.38323, 0.38533,
    0.38742, 0.38950, 0.39158, 0.39365, 0.39571, 0.39777, 0.39982, 0.40186,
    0.40389, 0.40592, 0.40794, 0.40996, 0.41197, 0.41397, 0.41596, 0.41795,
    0.41993, 0.42191, 0.42388, 0.42584, 0.42779, 0.42974, 0.43168, 0.43362,
    0.43554, 0.43747, 0.43938, 0.44129, 0.44319, 0.44509, 0.44698, 0.44886,
    0.45073, 0.45260, 0.45447, 0.45632, 0.45817, 0.46002, 0.46186, 0.46369,
    0.46551, 0.46733, 0.46914, 0.47095, 0.47275, 0.47454, 0.47633, 0.47811,
    0.47989, 0.48166, 0.48342, 0.48518, 0.48693, 0.48867, 0.49041, 0.49214,
    0.49387, 0.49559, 0.49730, 0.49901, 0.50072, 0.50241, 0.50410, 0.50579,
    0.50747, 0.50914, 0.51081, 0.51247, 0.51413, 0.51578, 0.51742, 0.51906,
    0.52069, 0.52232, 0.52394, 0.52556, 0.52717, 0.52878, 0.53038, 0.53197,
    0.53356, 0.53514, 0.53672, 0.53829, 0.53986, 0.54142, 0.54297, 0.54452,
    0.54607, 0.54761, 0.54914, 0.55067, 0.55220, 0.55371, 0.55523, 0.55673,
    0.55824, 0.55973, 0.56123, 0.56271, 0.56420, 0.56567, 0.56715, 0.56861,
    0.57007, 0.57153, 0.57298, 0.57443, 0.57587, 0.57731, 0.57874, 0.58017,
    0.58159, 0.58301, 0.58443, 0.58583, 0.58724, 0.58864, 0.59003, 0.59142,
    0.59281, 0.59419, 0.59556, 0.59694, 0.59830, 0.59966, 0.60102, 0.60238,
    0.60373, 0.60507, 0.60641, 0.60775, 0.60908, 0.61040, 0.61173, 0.61305,
    0.61436, 0.61567, 0.61698, 0.61828, 0.61957, 0.62087, 0.62216, 0.62344,
    0.62472, 0.62600, 0.62727, 0.62854, 0.62980, 0.63106, 0.63232, 0.63357,
    0.63482, 0.63606, 0.63730, 0.63854, 0.63977, 0.64100, 0.64222, 0.64344,
    0.64466, 0.64587, 0.64708, 0.64829, 0.64949, 0.65069, 0.65188, 0.65307,
    0.65426, 0.65544, 0.65662, 0.65779, 0.65897, 0.66013, 0.66130, 0.66246,
    0.66362, 0.66477, 0.66592, 0.66707, 0.66821, 0.66935, 0.67048, 0.67162,
    0.67275, 0.67387, 0.67499, 0.67611, 0.67723, 0.67834, 0.67945, 0.68055,
    0.68165, 0.68275, 0.68385, 0.68494, 0.68603, 0.68711, 0.68819, 0.68927,
    0.69035, 0.69142, 0.69249, 0.69355, 0.69461, 0.69567, 0.69673, 0.69778,
    0.69883, 0.69988, 0.70092, 0.70196, 0.70300, 0.70403, 0.70506, 0.70609,
    0.70711, 0.70813, 0.70915, 0.71017, 0.71118, 0.71219, 0.71319, 0.71420,
    0.71520, 0.71620, 0.71719, 0.71818, 0.71917, 0.72016, 0.72114, 0.72212,
    0.72309, 0.72407, 0.72504, 0.72601, 0.72697, 0.72794, 0.72890, 0.72985,
    0.73081, 0.73176, 0.73271, 0.73365, 0.73460, 0.73554, 0.73647, 0.73741,
    0.73834, 0.73927, 0.74020, 0.74112, 0.74204, 0.74296, 0.74388, 0.74479,
    0.74570, 0.74661, 0.74751, 0.74842, 0.74932, 0.75021, 0.75111, 0.75200,
    0.75289, 0.75378, 0.75466, 0.75555, 0.75643, 0.75730, 0.75818, 0.75905,
    0.75992, 0.76079, 0.76165, 0.76251, 0.76337, 0.76423, 0.76508, 0.76594,
    0.76679, 0.76763, 0.76848, 0.76932, 0.77016, 0.77100, 0.77183, 0.77267,
    0.77350, 0.77432, 0.77515, 0.77597, 0.77680, 0.77761, 0.77843, 0.77924,
    0.78006, 0.78087, 0.78167, 0.78248, 0.78328, 0.78408, 0.78488, 0.78568,
    0.78647, 0.78726, 0.78805, 0.78884, 0.78962, 0.79040, 0.79118, 0.79196,
    0.79274, 0.79351, 0.79428, 0.79505, 0.79582, 0.79658, 0.79735, 0.79811,
    0.79887, 0.79962, 0.80038, 0.80113, 0.80188, 0.80263, 0.80337, 0.80412,
    0.80486, 0.80560, 0.80634, 0.80707, 0.80780, 0.80854, 0.80926, 0.80999,
    0.81072, 0.81144, 0.81216, 0.81288, 0.81360, 0.81431, 0.81503, 0.81574,
    0.81645, 0.81715, 0.81786, 0.81856, 0.81926, 0.81996, 0.82066, 0.82135,
    0.82205, 0.82274, 0.82343, 0.82412, 0.82480, 0.82549, 0.82617, 0.82685,
    0.82753, 0.82820, 0.82888, 0.82955, 0.83022, 0.83089, 0.83155, 0.83222,
    0.83288, 0.83354, 0.83420, 0.83486, 0.83552, 0.83617, 0.83682, 0.83747,
    0.83812, 0.83877, 0.83941, 0.84005, 0.84069, 0.84133, 0.84197, 0.84261,
    0.84324, 0.84387, 0.84450, 0.84513, 0.84576, 0.84639, 0.84701, 0.84763,
    0.84825, 0.84887, 0.84949, 0.85010, 0.85071, 0.85132, 0.85193, 0.85254,
    0.85315, 0.85375, 0.85436, 0.85496, 0.85556, 0.85615, 0.85675, 0.85735,
    0.85794, 0.85853, 0.85912, 0.85971, 0.86029, 0.86088, 0.86146, 0.86204,
    0.86262, 0.86320, 0.86378, 0.86435, 0.86493, 0.86550, 0.86607, 0.86664,
    0.86720, 0.86777, 0.86833, 0.86889, 0.86945, 0.87001, 0.87057, 0.87113,
    0.87168, 0.87223, 0.87278, 0.87333, 0.87388, 0.87443, 0.87497, 0.87552,
    0.87606, 0.87660, 0.87714, 0.87768, 0.87821, 0.87875, 0.87928, 0.87981,
    0.88034, 0.88087, 0.88140, 0.88192, 0.88244, 0.88297, 0.88349, 0.88401,
    0.88453, 0.88504, 0.88556, 0.88607, 0.88658, 0.88709, 0.88760, 0.88811,
    0.88862, 0.88912, 0.88963, 0.89013, 0.89063, 0.89113, 0.89163, 0.89212,
    0.89262, 0.89311, 0.89360, 0.89409, 0.89458, 0.89507, 0.89556, 0.89604,
    0.89653, 0.89701, 0.89749, 0.89797, 0.89845, 0.89892, 0.89940, 0.89987,
    0.90035, 0.90082, 0.90129, 0.90176, 0.90222, 0.90269, 0.90316, 0.90362,
    0.90408, 0.90454, 0.90500, 0.90546, 0.90592, 0.90637, 0.90683, 0.90728,
    0.90773, 0.90818, 0.90863, 0.90908, 0.90952, 0.90997, 0.91041, 0.91085,
    0.91130, 0.91173, 0.91217, 0.91261, 0.91305, 0.91348, 0.91392, 0.91435,
    0.91478, 0.91521, 0.91564, 0.91606, 0.91649, 0.91691, 0.91734, 0.91776,
    0.91818, 0.91860, 0.91902, 0.91944, 0.91985, 0.92027, 0.92068, 0.92109,
    0.92150, 0.92191, 0.92232, 0.92273, 0.92314, 0.92354, 0.92395, 0.92435,
    0.92475, 0.92515, 0.92555, 0.92595, 0.92634, 0.92674, 0.92713, 0.92753,
    0.92792, 0.92831, 0.92870, 0.92909, 0.92947, 0.92986, 0.93025, 0.93063,
    0.93101, 0.93139, 0.93177, 0.93215, 0.93253, 0.93291, 0.93328, 0.93366,
    0.93403, 0.93440, 0.93478, 0.93515, 0.93551, 0.93588, 0.93625, 0.93661,
    0.93698, 0.93734, 0.93770, 0.93807, 0.93843, 0.93878, 0.93914, 0.93950,
    0.93986, 0.94021, 0.94056, 0.94092, 0.94127, 0.94162, 0.94197, 0.94231,
    0.94266, 0.94301, 0.94335, 0.94369, 0.94404, 0.94438, 0.94472, 0.94506,
    0.94540, 0.94573, 0.94607, 0.94641, 0.94674, 0.94707, 0.94740, 0.94774,
    0.94807, 0.94839, 0.94872, 0.94905, 0.94937, 0.94970, 0.95002, 0.95035,
    0.95067, 0.95099, 0.95131, 0.95163, 0.95194, 0.95226, 0.95257, 0.95289,
    0.95320, 0.95351, 0.95383, 0.95414, 0.95445, 0.95475, 0.95506, 0.95537,
    0.95567, 0.95598, 0.95628, 0.95658, 0.95688, 0.95718, 0.95748, 0.95778,
    0.95808, 0.95838, 0.95867, 0.95897, 0.95926, 0.95955, 0.95984, 0.96013,
    0.96042, 0.96071, 0.96100, 0.96129, 0.96157, 0.96186, 0.96214, 0.96242,
    0.96271, 0.96299, 0.96327, 0.96355, 0.96382, 0.96410, 0.96438, 0.96465,
    0.96493, 0.96520, 0.96547, 0.96574, 0.96602, 0.96629, 0.96655, 0.96682,
    0.96709, 0.96735, 0.96762, 0.96788, 0.96815, 0.96841, 0.96867, 0.96893,
    0.96919, 0.96945, 0.96971, 0.96996, 0.97022, 0.97047, 0.97073, 0.97098,
    0.97123, 0.97149, 0.97174, 0.97199, 0.97223, 0.97248, 0.97273, 0.97297,
    0.97322, 0.97346, 0.97371, 0.97395, 0.97419, 0.97443, 0.97467, 0.97491,
    0.97515, 0.97539, 0.97562, 0.97586, 0.97609, 0.97633, 0.97656, 0.97679,
    0.97702, 0.97725, 0.97748, 0.97771, 0.97794, 0.97817, 0.97839, 0.97862,
    0.97884, 0.97907, 0.97929, 0.97951, 0.97973, 0.97995, 0.98017, 0.98039,
    0.98061, 0.98082, 0.98104, 0.98125, 0.98147, 0.98168, 0.98189, 0.98211,
    0.98232, 0.98253, 0.98274, 0.98295, 0.98315, 0.98336, 0.98357, 0.98377,
    0.98398, 0.98418, 0.98438, 0.98458, 0.98478, 0.98498, 0.98518, 0.98538,
    0.98558, 0.98578, 0.98597, 0.98617, 0.98636, 0.98656, 0.98675, 0.98694,
    0.98714, 0.98733, 0.98752, 0.98771, 0.98789, 0.98808, 0.98827, 0.98845,
    0.98864, 0.98882, 0.98901, 0.98919, 0.98937, 0.98955, 0.98973, 0.98991,
    0.99009, 0.99027, 0.99045, 0.99063, 0.99080, 0.99098, 0.99115, 0.99133,
    0.99150, 0.99167, 0.99184, 0.99201, 0.99218, 0.99235, 0.99252, 0.99269,
    0.99285, 0.99302, 0.99319, 0.99335, 0.99351, 0.99368, 0.99384, 0.99400,
    0.99416, 0.99432, 0.99448, 0.99464, 0.99480, 0.99495, 0.99511, 0.99527,
    0.99542, 0.99558, 0.99573, 0.99588, 0.99603, 0.99619, 0.99634, 0.99649,
    0.99664, 0.99678, 0.99693, 0.99708, 0.99722, 0.99737, 0.99751, 0.99766,
    0.99780, 0.99794, 0.99809, 0.99823, 0.99837, 0.99851, 0.99865, 0.99879,
    0.99892, 0.99906, 0.99920, 0.99933, 0.99947, 0.99960, 0.99974, 0.99987,
    1.00000,
];

fn acr3(v: f32) -> f32 {
    let table = &ADOBE_CAMERA_RAW_DEFAULT_CURVE;
    let x = v.clamp(0.0, 1.0) * (table.len() - 1) as f32;
    let i = (x as usize).min(table.len() - 2);
    let f = x - i as f32;
    table[i] + (table[i + 1] - table[i]) * f
}

/// Where a brighter body's highlights start to roll off rather than clip.
///
/// A positive offset multiplies everything, so without this the top part of
/// the sensor's range - a quarter of a stop's worth for +0.3 EV - would be
/// pushed straight past white. Camera Raw keeps that detail, and so must this:
/// below the knee light is untouched, above it the sensor's own white is
/// brought down to exactly 1.
const SHOULDER_KNEE: f32 = 0.5;

/// Linear light, before the offset, to linear light out.
fn tone(v: f32, ev: f32) -> f32 {
    let v = v.clamp(0.0, 1.0);
    let gain = ev.exp2();
    let exposed = if ev >= 0.0 {
        shoulder(v * gain, gain)
    } else {
        // A darker body: the mid-tones come down by the offset and white
        // stays white, as the DNG SDK's exposure tone does for a negative
        // exposure. The cubic term is all but zero below the highlights:
        // middle grey sits within 2% of the bare offset at -0.5 EV.
        gain * v + (1.0 - gain) * v.powi(3)
    };
    acr3(exposed)
}

/// Untouched up to the knee, then compressed so that `white` lands on 1. Slope
/// one at the knee, so the join does not show.
fn shoulder(x: f32, white: f32) -> f32 {
    if white <= 1.0 || x <= SHOULDER_KNEE {
        return x.min(1.0);
    }
    let room_in = white - SHOULDER_KNEE;
    let room_out = 1.0 - SHOULDER_KNEE;
    let squeeze = (room_in - room_out) / (room_in * room_out);
    let above = x - SHOULDER_KNEE;
    (SHOULDER_KNEE + above / (1.0 + above * squeeze)).min(1.0)
}

/// The 16 points, on sRGB-encoded values as every RAW tone curve is stored.
///
/// Spaced as the 1.3rd power of an even step, so they crowd into the shadows,
/// where the curve bends most per code value. With these, their monotone spline
/// stays within 0.45/255 of the exact curve over the table's whole range of
/// offsets; evenly spaced points miss by 1.2/255 (test below).
pub fn curve_points(ev: f32) -> Vec<ToneCurvePoint> {
    (0..16)
        .map(|i| {
            let x = (i as f32 / 15.0).powf(1.3);
            ToneCurvePoint {
                x: x * 255.0,
                y: encode(tone(decode(x), ev)) * 255.0,
            }
        })
        .collect()
}

fn encode(v: f32) -> f32 {
    let v = v.clamp(0.0, 1.0);
    if v <= 0.0031308 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

fn decode(e: f32) -> f32 {
    if e <= 0.04045 {
        e / 12.92
    } else {
        ((e + 0.055) / 1.055).powf(2.4)
    }
}

// ---------------------------------------------------------------------------
// The exposure offset
// ---------------------------------------------------------------------------

/// What the offset is worked out from.
#[derive(Debug, Clone, Default)]
pub struct Facts {
    pub make: String,
    pub model: String,
    pub iso: Option<f32>,
    /// A DNG's own BaselineExposure, which outranks any table.
    pub dng_baseline: Option<f32>,
}

/// Where an offset came from, for the log.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Origin {
    /// The DNG said so.
    Dng,
    /// The body is in the table.
    Listed,
    /// The body is not, so the median of its make's rows.
    MakeMedian,
    /// Neither is the make, so the median of every row.
    TableMedian,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Baseline {
    pub ev: f32,
    pub origin: Origin,
}

/// Camera, ISO and a DNG's own offset, from the file's metadata alone.
pub fn facts(bytes: &[u8]) -> Result<Facts, String> {
    let source = rawler::rawsource::RawSource::new_from_slice(bytes);
    let decoder =
        rawler::get_decoder(&source).map_err(|e| format!("could not read the RAW: {e}"))?;
    let metadata = decoder
        .raw_metadata(&source, &rawler::decoders::RawDecodeParams::default())
        .map_err(|e| format!("could not read the RAW's metadata: {e}"))?;
    let exif = &metadata.exif;
    let iso = exif
        .iso_speed_ratings
        .map(f32::from)
        .or(exif.recommended_exposure_index.map(|v| v as f32))
        .or(exif.iso_speed.map(|v| v as f32))
        .filter(|iso| *iso > 0.0);
    Ok(Facts {
        make: metadata.make,
        model: metadata.model,
        iso,
        dng_baseline: dng_baseline_exposure(bytes),
    })
}

/// BaselineExposure from a DNG's first directory. None for anything that is
/// not a DNG - plenty of RAW formats are TIFF underneath, so the DNGVersion tag
/// is what decides, not the header.
fn dng_baseline_exposure(bytes: &[u8]) -> Option<f32> {
    use rawler::formats::tiff::GenericTiffReader;
    use rawler::formats::tiff::reader::TiffReader;
    use rawler::tags::DngTag;

    if bytes.len() < 8 || !(bytes.starts_with(b"II") || bytes.starts_with(b"MM")) {
        return None;
    }
    let tiff = GenericTiffReader::new_with_buffer(bytes, 0, 0, Some(1)).ok()?;
    let root = tiff.root_ifd();
    root.get_entry(DngTag::DNGVersion)?;
    let ev = root
        .get_entry(DngTag::BaselineExposure)?
        .value
        .get_f32(0)
        .ok()
        .flatten()?;
    ev.is_finite().then_some(ev)
}

/// The photo's offset: a DNG's own, else the table's.
pub fn baseline(facts: &Facts) -> Baseline {
    if let Some(ev) = facts.dng_baseline {
        return Baseline {
            ev,
            origin: Origin::Dng,
        };
    }
    baseline_in(rows(), facts)
}

/// One body of the table.
struct Row {
    make: String,
    /// The model and its other names.
    names: Vec<String>,
    ev: f32,
    /// The lowest native ISO, where extended settings lie below it.
    lowest_iso: Option<f32>,
}

/// The table, `dng_baseline.tsv`, read once.
fn rows() -> &'static [Row] {
    static ROWS: OnceLock<Vec<Row>> = OnceLock::new();
    ROWS.get_or_init(|| parse_rows(include_str!("dng_baseline.tsv")))
}

fn parse_rows(text: &str) -> Vec<Row> {
    text.lines()
        .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
        .filter_map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            let [make, names, ev, lowest_iso] = fields[..] else {
                return None;
            };
            Some(Row {
                make: make.to_string(),
                names: names.split('|').map(str::to_string).collect(),
                ev: ev.parse().ok()?,
                lowest_iso: lowest_iso.parse().ok(),
            })
        })
        .collect()
}

fn baseline_in(rows: &[Row], facts: &Facts) -> Baseline {
    let of_make = |row: &&Row| same_make(&row.make, &facts.make);
    let listed = rows.iter().filter(of_make).find(|row| {
        row.names.iter().any(|name| {
            same_model(name, &facts.model)
                || same_model(&format!("{} {}", row.make, name), &facts.model)
        })
    });
    if let Some(row) = listed {
        let mut ev = row.ev;
        // Below the body's lowest native ISO the camera exposes a stop more and
        // pulls the raw back; Camera Raw takes that stop off again.
        if let (Some(lowest), Some(iso)) = (row.lowest_iso, facts.iso)
            && iso < lowest - 1.0
        {
            ev -= 1.0;
        }
        return Baseline {
            ev,
            origin: Origin::Listed,
        };
    }
    // A body not listed is predicted better by its own make than by every row.
    if let Some(ev) = median(rows.iter().filter(of_make).map(|row| row.ev)) {
        return Baseline {
            ev,
            origin: Origin::MakeMedian,
        };
    }
    Baseline {
        ev: median(rows.iter().map(|row| row.ev)).unwrap_or(0.0),
        origin: Origin::TableMedian,
    }
}

/// Makers are written many ways: "NIKON CORPORATION", "Nikon"; "OM Digital",
/// "OM Digital Solutions". Letters and digits only, and one may lead the other.
fn same_make(a: &str, b: &str) -> bool {
    let squash = |s: &str| -> String {
        s.chars()
            .filter(char::is_ascii_alphanumeric)
            .map(|c| c.to_ascii_lowercase())
            .collect()
    };
    let (a, b) = (squash(a), squash(b));
    !a.is_empty() && !b.is_empty() && (a.starts_with(&b) || b.starts_with(&a))
}

fn median(values: impl Iterator<Item = f32>) -> Option<f32> {
    let mut values: Vec<f32> = values.collect();
    if values.is_empty() {
        return None;
    }
    values.sort_by(f32::total_cmp);
    let n = values.len();
    Some(if n % 2 == 1 {
        values[n / 2]
    } else {
        (values[n / 2 - 1] + values[n / 2]) / 2.0
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Their `apply_curve` (shader.wgsl), line for line: what the GPU will do
    /// with the 16 points.
    pub(super) fn apply_curve(val: f32, points: &[ToneCurvePoint]) -> f32 {
        let n = points.len();
        let x = val * 255.0;
        if x <= points[0].x {
            return points[0].y / 255.0;
        }
        if x >= points[n - 1].x {
            return points[n - 1].y / 255.0;
        }
        for i in 0..n - 1 {
            let (p1, p2) = (points[i], points[i + 1]);
            if x <= p2.x {
                let p0 = points[i.saturating_sub(1)];
                let p3 = points[(i + 2).min(n - 1)];
                let before = (p1.y - p0.y) / (p1.x - p0.x).max(0.001);
                let current = (p2.y - p1.y) / (p2.x - p1.x).max(0.001);
                let after = (p3.y - p2.y) / (p3.x - p2.x).max(0.001);
                let m1 = if i == 0 {
                    current
                } else if before * current <= 0.0 {
                    0.0
                } else {
                    (before + current) / 2.0
                };
                let m2 = if i == n - 2 {
                    current
                } else if current * after <= 0.0 {
                    0.0
                } else {
                    (current + after) / 2.0
                };
                let dx = p2.x - p1.x;
                let t = (x - p1.x) / dx;
                let (t2, t3) = (t * t, t * t * t);
                let y = (2.0 * t3 - 3.0 * t2 + 1.0) * p1.y
                    + (t3 - 2.0 * t2 + t) * m1 * dx
                    + (-2.0 * t3 + 3.0 * t2) * p2.y
                    + (t3 - t2) * m2 * dx;
                return y / 255.0;
            }
        }
        points[n - 1].y / 255.0
    }

    fn worst_miss(ev: f32) -> f32 {
        let points = curve_points(ev);
        (0..=4000)
            .map(|k| {
                let e = k as f32 / 4000.0;
                (apply_curve(e, &points) - encode(tone(decode(e), ev))).abs() * 255.0
            })
            .fold(0.0, f32::max)
    }

    #[test]
    fn sixteen_points_carry_the_curve_within_half_a_code_value() {
        for ev in [-0.5, -0.15, 0.0, 0.3, 0.5, 0.85] {
            let miss = worst_miss(ev);
            assert!(miss < 0.5, "{ev:+} EV: off by {miss:.2}/255");
        }
    }

    #[test]
    fn middle_grey_lands_where_adobe_puts_it() {
        assert!((acr3(0.18) - 0.388).abs() < 0.001, "{}", acr3(0.18));
        assert!((tone(0.18, 0.0) - acr3(0.18)).abs() < 1e-6);
        // An offset is an exposure: +1 EV at middle grey reads the curve at 0.36.
        assert!((tone(0.18, 1.0) - acr3(0.36)).abs() < 1e-6);
    }

    #[test]
    fn black_and_white_stay_put_whatever_the_offset() {
        for ev in [-0.5, 0.0, 0.35, 0.85] {
            assert_eq!(tone(0.0, ev), 0.0);
            assert!((tone(1.0, ev) - 1.0).abs() < 1e-5, "{ev:+} EV white");
            let points = curve_points(ev);
            assert_eq!((points[0].x, points[0].y), (0.0, 0.0));
            assert!((points[15].y - 255.0).abs() < 0.01);
        }
    }

    #[test]
    fn the_curve_only_ever_rises() {
        for ev in [-0.5, 0.0, 0.85] {
            let mut last = -1.0;
            for k in 0..=1000 {
                let y = tone(k as f32 / 1000.0, ev);
                assert!(y >= last, "{ev:+} EV falls at {k}");
                last = y;
            }
            assert!(
                curve_points(ev)
                    .windows(2)
                    .all(|w| w[0].x < w[1].x && w[0].y <= w[1].y)
            );
        }
    }

    #[test]
    fn the_shoulder_joins_smoothly_and_ends_on_white() {
        let white = 0.85f32.exp2();
        assert_eq!(shoulder(SHOULDER_KNEE, white), SHOULDER_KNEE);
        let slope = (shoulder(SHOULDER_KNEE + 1e-3, white) - SHOULDER_KNEE) / 1e-3;
        assert!((slope - 1.0).abs() < 0.01, "slope {slope}");
        assert!((shoulder(white, white) - 1.0).abs() < 1e-6);
    }

    fn camera(make: &str, model: &str) -> Facts {
        Facts {
            make: make.into(),
            model: model.into(),
            ..Default::default()
        }
    }

    #[test]
    fn the_table_reads_and_finds_bodies_by_any_of_their_names() {
        assert_eq!(rows().len(), 230);
        let r6 = baseline(&camera("Canon", "EOS R6 Mark III"));
        assert_eq!(
            r6,
            Baseline {
                ev: 0.5,
                origin: Origin::Listed
            }
        );
        // An alias, and a maker written the other way.
        let t3 = baseline(&camera("CANON", "EOS REBEL T3"));
        assert_eq!(
            t3,
            Baseline {
                ev: -0.15,
                origin: Origin::Listed
            }
        );
        // A model reported with its maker in front.
        let r6 = baseline(&camera("Canon", "Canon EOS R6 Mark III"));
        assert_eq!(r6.origin, Origin::Listed);
    }

    #[test]
    fn a_body_never_takes_another_bodys_row() {
        // EOS R6 is listed, and is not EOS R6 Mark III.
        assert_eq!(baseline(&camera("Canon", "EOS R6")).ev, 0.35);
        assert_eq!(baseline(&camera("Canon", "EOS R6 Mark III")).ev, 0.5);
    }

    #[test]
    fn unlisted_bodies_take_their_makes_median_then_the_tables() {
        let canon = baseline(&camera("Canon", "EOS 9999D"));
        assert_eq!(canon.origin, Origin::MakeMedian);
        let nobody = baseline(&camera("Nobody", "X1"));
        assert_eq!(
            nobody,
            Baseline {
                ev: 0.3,
                origin: Origin::TableMedian
            }
        );
    }

    #[test]
    fn extended_low_iso_takes_a_stop_off() {
        let at = |iso: f32| {
            baseline(&Facts {
                iso: Some(iso),
                ..camera("Sony", "ILCE-7M3")
            })
            .ev
        };
        assert_eq!(at(100.0), 0.35);
        assert_eq!(at(50.0), 0.35 - 1.0);
    }

    #[test]
    fn a_dngs_own_offset_outranks_the_table() {
        let facts = Facts {
            dng_baseline: Some(-0.25),
            ..camera("Canon", "EOS R6 Mark III")
        };
        assert_eq!(
            baseline(&facts),
            Baseline {
                ev: -0.25,
                origin: Origin::Dng
            }
        );
    }

    /// A little-endian TIFF with one directory holding the given entries, each
    /// (tag, type, count, value-or-offset). Values that do not fit in four
    /// bytes go after the directory.
    fn tiff(entries: &[(u16, u16, u32, Vec<u8>)]) -> Vec<u8> {
        let mut out = b"II*\0".to_vec();
        out.extend(8u32.to_le_bytes());
        out.extend((entries.len() as u16).to_le_bytes());
        let mut data_at = 8 + 2 + 12 * entries.len() as u32 + 4;
        let mut tail: Vec<u8> = Vec::new();
        for (tag, kind, count, value) in entries {
            out.extend(tag.to_le_bytes());
            out.extend(kind.to_le_bytes());
            out.extend(count.to_le_bytes());
            if value.len() <= 4 {
                let mut v = value.clone();
                v.resize(4, 0);
                out.extend(v);
            } else {
                out.extend(data_at.to_le_bytes());
                data_at += value.len() as u32;
                tail.extend(value);
            }
        }
        out.extend(0u32.to_le_bytes());
        out.extend(tail);
        out
    }

    #[test]
    fn baseline_exposure_is_read_from_a_dng_and_only_a_dng() {
        let version = (50706u16, 1u16, 4u32, vec![1, 4, 0, 0]);
        // SRATIONAL -1/2.
        let mut half = (-1i32).to_le_bytes().to_vec();
        half.extend(2i32.to_le_bytes());
        let exposure = (50730u16, 10u16, 1u32, half);
        assert_eq!(
            dng_baseline_exposure(&tiff(&[version.clone(), exposure.clone()])),
            Some(-0.5)
        );
        // The same tag in a TIFF that is not a DNG says nothing.
        assert_eq!(dng_baseline_exposure(&tiff(&[exposure])), None);
        // A DNG without the tag, and something that is not a TIFF at all.
        assert_eq!(dng_baseline_exposure(&tiff(&[version])), None);
        assert_eq!(dng_baseline_exposure(b"not a tiff at all"), None);
    }
}

/// Through the real shader, on a GPU: `cargo test --lib dng_standard::gpu -- --ignored`.
#[cfg(test)]
mod gpu {
    use super::*;
    use crate::image_processing::GpuContext;
    use image::{DynamicImage, ImageBuffer, Rgba};
    use std::sync::{Arc, Mutex};

    fn device() -> Option<GpuContext> {
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: None,
        }))
        .ok()?;
        eprintln!("adapter: {:?}", adapter.get_info());
        let limits = adapter.limits();
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("dng standard test"),
            required_features: wgpu::Features::empty(),
            required_limits: limits.clone(),
            experimental_features: wgpu::ExperimentalFeatures::default(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        }))
        .ok()?;
        Some(GpuContext {
            device: Arc::new(device),
            queue: Arc::new(queue),
            limits,
            display: Arc::new(Mutex::new(None)),
        })
    }

    /// Their whole render, as an export makes it.
    fn render(
        ctx: &GpuContext,
        image: &DynamicImage,
        js: serde_json::Value,
        as_shot: crate::white_balance::WhiteBalance,
    ) -> image::Rgba32FImage {
        let adjustments =
            crate::image_processing::get_all_adjustments_from_json(&js, true, as_shot, None, None);
        crate::mods::export_precision::render_high_precision(
            ctx,
            image,
            crate::gpu_processing::RenderRequest {
                adjustments,
                mask_bitmaps: &[],
                lut: None,
                roi: None,
            },
        )
        .expect("renders")
        .to_rgba32f()
    }

    const PATCH: u32 = 8;

    /// Greys from black to white, and saturated colours at three levels.
    fn patches() -> Vec<[f32; 3]> {
        let mut out: Vec<[f32; 3]> = [0.0, 0.01, 0.05, 0.18, 0.4, 0.7, 1.0]
            .iter()
            .map(|&v| [v; 3])
            .collect();
        for level in [0.05, 0.2, 0.6] {
            for c in [
                [1.0, 0.0, 0.0],
                [1.0, 0.45, 0.05],
                [0.9, 0.85, 0.1],
                [0.1, 0.8, 0.15],
                [0.1, 0.35, 1.0],
                [0.75, 0.2, 0.8],
                [0.8, 0.55, 0.4],
            ] {
                out.push(c.map(|v: f32| v * level));
            }
        }
        out
    }

    fn image_of(patches: &[[f32; 3]]) -> DynamicImage {
        let w = PATCH * patches.len() as u32;
        DynamicImage::ImageRgba32F(ImageBuffer::from_fn(w, PATCH, |x, _| {
            let [r, g, b] = patches[(x / PATCH) as usize];
            Rgba([r, g, b, 1.0])
        }))
    }

    fn centre(out: &image::Rgba32FImage, i: usize) -> [f32; 3] {
        let p = out.get_pixel(i as u32 * PATCH + PATCH / 2, PATCH / 2);
        [p[0], p[1], p[2]]
    }

    fn mul(m: [[f32; 3]; 3], c: [f32; 3]) -> [f32; 3] {
        m.map(|row| row[0] * c[0] + row[1] * c[1] + row[2] * c[2])
    }

    /// DNG Standard on the CPU, with the exact curve rather than its 16 points.
    fn reference(c: [f32; 3], ev: f32) -> [f32; 3] {
        const TO: [[f32; 3]; 3] = [
            [0.5293458, 0.3300728, 0.1405813],
            [0.0983744, 0.873461, 0.0281647],
            [0.0168832, 0.1176725, 0.8654443],
        ];
        const FROM: [[f32; 3]; 3] = [
            [2.0340764, -0.7273343, -0.3067417],
            [-0.2288134, 1.2317302, -0.0029169],
            [-0.0085698, -0.1532866, 1.1618564],
        ];
        let pp = mul(TO, c).map(|v| v.clamp(0.0, 1.0));
        let hi = pp[0].max(pp[1]).max(pp[2]);
        let lo = pp[0].min(pp[1]).min(pp[2]);
        let (hi_t, lo_t) = (tone(hi, ev), tone(lo, ev));
        let toned = if hi - lo <= 1e-6 {
            [hi_t; 3]
        } else {
            pp.map(|v| lo_t + (v - lo) * (hi_t - lo_t) / (hi - lo))
        };
        mul(FROM, toned).map(encode)
    }

    fn worst(got: &image::Rgba32FImage, want: impl Fn([f32; 3]) -> [f32; 3]) -> f32 {
        let mut worst = 0.0f32;
        for (i, c) in patches().into_iter().enumerate() {
            let (g, w) = (centre(got, i), want(c));
            let miss = (0..3).map(|k| (g[k] - w[k]).abs()).fold(0.0, f32::max) * 255.0;
            if miss > 1.0 {
                eprintln!("patch {i} {c:?}: shader {g:?}, expected {w:?} ({miss:.2}/255)");
            }
            worst = worst.max(miss);
        }
        worst
    }

    fn json_points(points: &[ToneCurvePoint]) -> serde_json::Value {
        serde_json::to_value(points).unwrap()
    }

    #[test]
    #[ignore = "needs a GPU; run with --ignored"]
    fn the_shader_renders_dng_standard_as_the_reference_does() {
        let ctx = device().expect("no wgpu adapter");
        let image = image_of(&patches());
        for ev in [-0.5, 0.0, 0.5] {
            let js = serde_json::json!({
                "rawToneRendering": MODE,
                "rawToneCurve": json_points(&curve_points(ev)),
                "agSharpen": { "capture": false, "amount": 0 },
            });
            let got = render(
                &ctx,
                &image,
                js,
                crate::white_balance::WhiteBalance::reference(),
            );
            let miss = worst(&got, |c| reference(c, ev));
            eprintln!("{ev:+} EV: worst {miss:.2}/255");
            assert!(
                miss < 1.5,
                "{ev:+} EV: the shader is {miss:.2}/255 from the reference"
            );
        }
    }

    /// A photo nobody has edited is rendered from no adjustments at all, in
    /// thumbnails and exports as in the editor: DNG Standard with its Sharpen,
    /// exactly as if both had been chosen by hand.
    #[test]
    #[ignore = "needs a GPU; run with --ignored"]
    fn an_unedited_raw_renders_as_dng_standard_with_its_sharpen() {
        let ctx = device().expect("no wgpu adapter");
        let image = image_of(&patches());
        let wb = crate::white_balance::WhiteBalance::reference();
        let unedited = render(&ctx, &image, serde_json::Value::Null, wb);
        let chosen = render(
            &ctx,
            &image,
            serde_json::json!({
                "rawToneRendering": MODE,
                "rawToneCurve": json_points(&curve_for_photo(None)),
                "agSharpen": {
                    "amount": crate::mods::sharpen::DNG_STANDARD_AMOUNT,
                    "usmRadius": crate::mods::sharpen::DNG_STANDARD_USM_RADIUS,
                },
            }),
            wb,
        );
        assert_eq!(unedited.as_raw(), chosen.as_raw());
        let plain = render(
            &ctx,
            &image,
            serde_json::json!({ "rawToneRendering": "default" }),
            wb,
        );
        assert_ne!(
            unedited.as_raw(),
            plain.as_raw(),
            "and it is not RapidRAW's rendering"
        );
    }

    /// Moving the RAW tone renderings out of their main() must not move a
    /// pixel of the ones that were there: their own raw branch with no mode
    /// set, and Base Curve's per-channel curve.
    #[test]
    #[ignore = "needs a GPU; run with --ignored"]
    fn the_other_renderings_are_what_they_were() {
        let ctx = device().expect("no wgpu adapter");
        let image = image_of(&patches());
        let wb = crate::white_balance::WhiteBalance::reference();

        let theirs = render(
            &ctx,
            &image,
            serde_json::json!({ "rawToneRendering": "default", "agSharpen": { "capture": false } }),
            wb,
        );
        let miss = worst(&theirs, |c| {
            c.map(|v| {
                let s = encode(v).powf(1.0 / 1.1);
                let curve = s * s * (3.0 - 2.0 * s);
                s + (curve - s) * 0.75
            })
        });
        assert!(miss < 1.0, "their raw branch moved by {miss:.2}/255");

        let base = crate::mods::raw_tone::base_curve();
        let js = serde_json::json!({
            "rawToneRendering": "baseCurve",
            "rawToneCurve": json_points(&base),
            "agSharpen": { "capture": false, "amount": 0 },
        });
        let got = render(&ctx, &image, js, wb);
        let miss = worst(&got, |c| {
            c.map(|v| super::tests::apply_curve(encode(v), &base))
        });
        assert!(miss < 1.0, "Base Curve moved by {miss:.2}/255");
    }

    /// Sheets to judge by eye. Each RAW in AG_RAWS (separated by ';') rendered
    /// with RapidRAW's default and with DNG Standard, beside the camera's own
    /// JPEG, written to AG_OUT as sheet.html. Capture sharpening is off in both,
    /// so tone is the only difference.
    #[test]
    #[ignore = "reads RAW files; run by hand"]
    fn sheets() {
        use base64::Engine;
        let raws = std::env::var("AG_RAWS").expect("set AG_RAWS");
        let out = std::path::PathBuf::from(std::env::var("AG_OUT").expect("set AG_OUT"));
        let width: u32 = std::env::var("AG_WIDTH")
            .ok()
            .and_then(|w| w.parse().ok())
            .unwrap_or(900);
        std::fs::create_dir_all(&out).unwrap();
        let ctx = device().expect("no wgpu adapter");
        let jpeg = |img: &DynamicImage| {
            let mut bytes = Vec::new();
            DynamicImage::ImageRgb8(img.to_rgb8())
                .write_to(
                    &mut std::io::Cursor::new(&mut bytes),
                    image::ImageFormat::Jpeg,
                )
                .unwrap();
            base64::engine::general_purpose::STANDARD.encode(bytes)
        };
        let mut html = String::from(
            "<!doctype html><meta charset=utf-8><title>DNG Standard sheets</title><style>\
             body{background:#1b1b1b;color:#ddd;font:14px system-ui;margin:16px}\
             .row{display:flex;gap:8px;margin:0 0 28px}figure{margin:0;flex:1}\
             img{width:100%;display:block}figcaption{padding:4px 0}h2{font-size:15px;margin:8px 0}\
             </style>",
        );
        for (n, path) in raws.split(';').filter(|p| !p.trim().is_empty()).enumerate() {
            let bytes = std::fs::read(path).expect("read raw");
            let linear = crate::raw_processing::develop_raw_image(
                &bytes,
                false,
                2.5,
                "default".to_string(),
                None,
                Some(path),
            )
            .expect("decodes");
            let h = (linear.height() as f32 * width as f32 / linear.width() as f32).round() as u32;
            let small = crate::image_processing::downscale_f32_image(&linear, width, h);
            let wb = crate::raw_processing::read_as_shot_white_balance(&bytes)
                .unwrap_or_else(crate::white_balance::WhiteBalance::reference);
            let facts = facts(&bytes).expect("metadata");
            let base = baseline(&facts);
            let off = serde_json::json!({ "agSharpen": { "capture": false, "amount": 0 } });
            let dng = serde_json::json!({
                "rawToneRendering": MODE,
                "rawToneCurve": json_points(&curve_points(base.ev)),
                "agSharpen": { "capture": false, "amount": 0 },
            });
            let a = DynamicImage::ImageRgba32F(render(&ctx, &small, off, wb));
            let b = DynamicImage::ImageRgba32F(render(&ctx, &small, dng, wb));
            let camera = crate::image_loader::embedded_preview_fallback(&bytes)
                .map(|j| j.resize(width, h * 2, image::imageops::FilterType::Lanczos3));
            let name = std::path::Path::new(path)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string();
            html += &format!(
                "<h2>{} - {} {}, ISO {}</h2><div class=row>",
                n + 1,
                facts.make,
                facts.model,
                facts
                    .iso
                    .map(|i| (i as u32).to_string())
                    .unwrap_or_default()
            );
            html += &format!(
                "<figure><img src='data:image/jpeg;base64,{}'><figcaption>A - Default (RapidRAW)</figcaption></figure>",
                jpeg(&a)
            );
            html += &format!(
                "<figure><img src='data:image/jpeg;base64,{}'><figcaption>B - DNG Standard, offset {:+.2} EV ({:?})</figcaption></figure>",
                jpeg(&b),
                base.ev,
                base.origin
            );
            if let Some(camera) = camera {
                html += &format!(
                    "<figure><img src='data:image/jpeg;base64,{}'><figcaption>C - Camera JPEG (reference)</figcaption></figure>",
                    jpeg(&camera)
                );
            }
            html += "</div>";
            eprintln!("{name}: {:+.2} EV ({:?})", base.ev, base.origin);
        }
        std::fs::write(out.join("sheet.html"), html).unwrap();
    }
    /// The most detailed window of a photo, where sharpening shows: the
    /// `w` x `h` area with the most luminance gradient, searched at an eighth
    /// of the size.
    fn busiest(image: &DynamicImage, w: u32, h: u32) -> (u32, u32) {
        let (iw, ih) = (image.width(), image.height());
        let small = crate::image_processing::downscale_f32_image(image, iw / 8, ih / 8).to_rgb32f();
        let luma = |x: u32, y: u32| {
            let p = small.get_pixel(x.min(small.width() - 1), y.min(small.height() - 1));
            encode(0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2])
        };
        let (sw, sh) = (w / 8, h / 8);
        let mut best = (0.0f32, 0, 0);
        for y0 in (0..small.height().saturating_sub(sh)).step_by(4) {
            for x0 in (0..small.width().saturating_sub(sw)).step_by(4) {
                let mut energy = 0.0;
                for y in y0..y0 + sh {
                    for x in x0..x0 + sw {
                        energy += (luma(x + 1, y) - luma(x, y)).abs()
                            + (luma(x, y + 1) - luma(x, y)).abs();
                    }
                }
                if energy > best.0 {
                    best = (energy, x0 * 8, y0 * 8);
                }
            }
        }
        (best.1, best.2)
    }

    /// Sharpening sheets at 200%. Each RAW in AG_RAWS, cropped at full size to
    /// its most detailed area and rendered with DNG Standard and capture
    /// sharpening on, once per Sharpen setting in AG_SHARPEN
    /// ("amount/radius" separated by ';', amount 0 being today's default).
    #[test]
    #[ignore = "reads RAW files; run by hand"]
    fn sharpening_sheets() {
        use base64::Engine;
        let raws = std::env::var("AG_RAWS").expect("set AG_RAWS");
        let out = std::path::PathBuf::from(std::env::var("AG_OUT").expect("set AG_OUT"));
        let settings: Vec<(f32, f32)> = std::env::var("AG_SHARPEN")
            .unwrap_or_else(|_| "0/2;50/2;100/2".into())
            .split(';')
            .map(|s| {
                let (a, r) = s.split_once('/').expect("amount/radius");
                (a.parse().unwrap(), r.parse().unwrap())
            })
            .collect();
        std::fs::create_dir_all(&out).unwrap();
        let ctx = device().expect("no wgpu adapter");
        // Rendered with a margin, shown from the middle, twice the size.
        let (cw, ch, show_w, show_h) = (720u32, 560u32, 360u32, 280u32);
        let mut html = String::from(
            "<!doctype html><meta charset=utf-8><title>Sharpening sheets</title><style>\
             body{background:#1b1b1b;color:#ddd;font:14px system-ui;margin:16px}\
             .row{display:flex;gap:8px;margin:0 0 28px}figure{margin:0}\
             img{width:720px;height:560px;image-rendering:pixelated;display:block}\
             figcaption{padding:4px 0}h2{font-size:15px;margin:8px 0}</style>",
        );
        for (n, path) in raws.split(';').filter(|p| !p.trim().is_empty()).enumerate() {
            let bytes = std::fs::read(path).expect("read raw");
            let linear = crate::raw_processing::develop_raw_image(
                &bytes,
                false,
                2.5,
                "default".to_string(),
                None,
                Some(path),
            )
            .expect("decodes");
            let (x, y) = busiest(&linear, cw, ch);
            let crop = linear.crop_imm(x, y, cw, ch);
            let wb = crate::raw_processing::read_as_shot_white_balance(&bytes)
                .unwrap_or_else(crate::white_balance::WhiteBalance::reference);
            let base = baseline(&facts(&bytes).expect("metadata"));
            let radius = crate::mods::sharpen::auto_radius(path);
            html += &format!(
                "<h2>{} - {} (capture radius {:?})</h2><div class=row>",
                n + 1,
                std::path::Path::new(path)
                    .file_name()
                    .unwrap()
                    .to_string_lossy(),
                radius
            );
            for (i, &(amount, usm_radius)) in settings.iter().enumerate() {
                let js = serde_json::json!({
                    "rawToneRendering": MODE,
                    "rawToneCurve": json_points(&curve_points(base.ev)),
                    "agSharpen": { "amount": amount, "usmRadius": usm_radius },
                });
                let got = DynamicImage::ImageRgba32F(render(&ctx, &crop, js, wb));
                let shown = got
                    .crop_imm((cw - show_w) / 2, (ch - show_h) / 2, show_w, show_h)
                    .to_rgb8();
                let mut bytes = Vec::new();
                DynamicImage::ImageRgb8(shown)
                    .write_to(
                        &mut std::io::Cursor::new(&mut bytes),
                        image::ImageFormat::Png,
                    )
                    .unwrap();
                let label = if amount == 0.0 {
                    "today: capture sharpening only".to_string()
                } else {
                    format!("+ Sharpen {amount}, radius {usm_radius}")
                };
                html += &format!(
                    "<figure><img src='data:image/png;base64,{}'><figcaption>{} - {label}</figcaption></figure>",
                    base64::engine::general_purpose::STANDARD.encode(bytes),
                    (b'A' + i as u8) as char,
                );
            }
            html += "</div>";
        }
        std::fs::write(out.join("sharpening.html"), html).unwrap();
    }
    /// Luma of an image at 1/`step` of its size, normalised to mean 0 and
    /// spread 1, so renders with different tone can be lined up.
    fn normalised_luma(image: &DynamicImage, step: u32) -> (Vec<f32>, u32, u32) {
        let rgb = image.to_rgb32f();
        let (w, h) = (rgb.width() / step, rgb.height() / step);
        let mut v: Vec<f32> = (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .map(|(x, y)| {
                let p = rgb.get_pixel(x * step, y * step);
                0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2]
            })
            .collect();
        let mean = v.iter().sum::<f32>() / v.len() as f32;
        let spread = (v.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / v.len() as f32).sqrt();
        v.iter_mut()
            .for_each(|x| *x = (*x - mean) / spread.max(1e-6));
        (v, w, h)
    }

    /// Where `theirs` sits in `ours`' frame: the rotation and offset that best
    /// match, searched at an eighth of the size and refined at full size.
    fn line_up(ours: &DynamicImage, theirs: &DynamicImage) -> (DynamicImage, i32, i32) {
        let (a, aw, ah) = normalised_luma(ours, 8);
        let mut best = (f32::MAX, theirs.clone(), 0i32, 0i32);
        for candidate in [
            theirs.clone(),
            theirs.rotate90(),
            theirs.rotate270(),
            theirs.rotate180(),
        ] {
            let (cw, ch) = (candidate.width() as f32, candidate.height() as f32);
            if (cw / ours.width() as f32 - 1.0).abs() > 0.05
                || (ch / ours.height() as f32 - 1.0).abs() > 0.05
            {
                continue;
            }
            let (b, bw, bh) = normalised_luma(&candidate, 8);
            for dy in -12i32..=12 {
                for dx in -12i32..=12 {
                    let (mut sum, mut n) = (0.0f32, 0u32);
                    for y in (ah / 4..ah * 3 / 4).step_by(2) {
                        for x in (aw / 4..aw * 3 / 4).step_by(2) {
                            let (bx, by) = (x as i32 + dx, y as i32 + dy);
                            if bx < 0 || by < 0 || bx >= bw as i32 || by >= bh as i32 {
                                continue;
                            }
                            sum += (a[(y * aw + x) as usize]
                                - b[(by as u32 * bw + bx as u32) as usize])
                                .abs();
                            n += 1;
                        }
                    }
                    let miss = sum / n.max(1) as f32;
                    if miss < best.0 {
                        best = (miss, candidate.clone(), dx * 8, dy * 8);
                    }
                }
            }
        }
        (best.1, best.2, best.3)
    }

    /// Refine an offset at full size over one window: the best of +-8 px.
    fn refine(
        ours: &DynamicImage,
        theirs: &DynamicImage,
        (x, y, w, h): (u32, u32, u32, u32),
        (dx, dy): (i32, i32),
    ) -> (i32, i32) {
        let a = normalised_luma(&ours.crop_imm(x, y, w, h), 1).0;
        let mut best = (f32::MAX, dx, dy);
        for ey in -8..=8 {
            for ex in -8..=8 {
                let (tx, ty) = (x as i32 + dx + ex, y as i32 + dy + ey);
                if tx < 0
                    || ty < 0
                    || tx as u32 + w > theirs.width()
                    || ty as u32 + h > theirs.height()
                {
                    continue;
                }
                let b = normalised_luma(&theirs.crop_imm(tx as u32, ty as u32, w, h), 1).0;
                let miss = a.iter().zip(&b).map(|(p, q)| (p - q).abs()).sum::<f32>();
                if miss < best.0 {
                    best = (miss, dx + ex, dy + ey);
                }
            }
        }
        (best.1, best.2)
    }

    /// Mean absolute Laplacian of sRGB-encoded luma, x100: how much fine
    /// edge contrast a crop holds. Higher is crisper (and noisier).
    fn crispness(image: &DynamicImage) -> f32 {
        let rgb = image.to_rgb32f();
        let l = |x: u32, y: u32| {
            let p = rgb.get_pixel(x, y);
            0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2]
        };
        let (w, h) = (rgb.width(), rgb.height());
        let mut sum = 0.0;
        for y in 1..h - 1 {
            for x in 1..w - 1 {
                sum +=
                    (4.0 * l(x, y) - l(x - 1, y) - l(x + 1, y) - l(x, y - 1) - l(x, y + 1)).abs();
            }
        }
        100.0 * sum / ((w - 2) * (h - 2)) as f32
    }

    /// One photo at 200% against RAWmakase's render of it and the camera's
    /// JPEG. AG_RAW the RAW, AG_REF RAWmakase's full-size render
    /// (`rawmakase render`), AG_OUT the folder, AG_CROP "x,y,w,h" in
    /// Argentum's frame (else the busiest area).
    #[test]
    #[ignore = "reads RAW files; run by hand"]
    fn against_rawmakase() {
        use base64::Engine;
        let path = std::env::var("AG_RAW").expect("set AG_RAW");
        let reference =
            image::open(std::env::var("AG_REF").expect("set AG_REF")).expect("reads AG_REF");
        let out = std::path::PathBuf::from(std::env::var("AG_OUT").expect("set AG_OUT"));
        let ctx = device().expect("no wgpu adapter");
        let bytes = std::fs::read(&path).expect("read raw");
        let linear = crate::raw_processing::develop_raw_image(
            &bytes,
            false,
            2.5,
            "default".to_string(),
            None,
            Some(&path),
        )
        .expect("decodes");
        let wb = crate::raw_processing::read_as_shot_white_balance(&bytes)
            .unwrap_or_else(crate::white_balance::WhiteBalance::reference);
        let base = baseline(&facts(&bytes).expect("metadata"));
        eprintln!(
            "ours {}x{}, RAWmakase {}x{}",
            linear.width(),
            linear.height(),
            reference.width(),
            reference.height()
        );
        let today = DynamicImage::ImageRgba32F(render(&ctx, &linear, serde_json::json!({}), wb));
        let dng = DynamicImage::ImageRgba32F(render(
            &ctx,
            &linear,
            serde_json::json!({ "rawToneRendering": MODE, "rawToneCurve": json_points(&curve_points(base.ev)) }),
            wb,
        ));
        let (w, h) = (360u32, 280u32);
        let (x, y) = match std::env::var("AG_CROP") {
            Ok(c) => {
                let v: Vec<u32> = c.split(',').map(|s| s.trim().parse().unwrap()).collect();
                (v[0], v[1])
            }
            Err(_) => busiest(&today, w, h),
        };
        eprintln!("crop at {x},{y}");
        let overview = crate::image_processing::downscale_f32_image(
            &today,
            today.width() / 8,
            today.height() / 8,
        );
        DynamicImage::ImageRgb8(overview.to_rgb8())
            .save(out.join("overview.png"))
            .unwrap();
        let mut columns: Vec<(String, DynamicImage)> = vec![
            (
                "Argentum today (capture sharpening)".into(),
                today.crop_imm(x, y, w, h),
            ),
            (
                "Argentum, DNG Standard (with its Sharpen 25)".into(),
                dng.crop_imm(x, y, w, h),
            ),
        ];
        // Extra columns: DNG Standard with Argentum's Sharpen, "amount/radius;...".
        for setting in std::env::var("AG_SHARPEN")
            .unwrap_or_default()
            .split(';')
            .filter(|s| !s.is_empty())
        {
            let (amount, radius) = setting.split_once('/').expect("amount/radius");
            let (amount, radius): (f32, f32) = (amount.parse().unwrap(), radius.parse().unwrap());
            let js = serde_json::json!({
                "rawToneRendering": MODE,
                "rawToneCurve": json_points(&curve_points(base.ev)),
                "agSharpen": { "amount": amount, "usmRadius": radius },
            });
            let got = DynamicImage::ImageRgba32F(render(&ctx, &linear, js, wb));
            columns.push((
                format!("DNG Standard + Sharpen {amount}, radius {radius}"),
                got.crop_imm(x, y, w, h),
            ));
        }
        let camera = crate::image_loader::embedded_preview_fallback(&bytes);
        for (label, theirs) in [
            ("RAWmakase 0.3.3 default".to_string(), Some(reference)),
            ("Camera JPEG".to_string(), camera),
        ] {
            let Some(theirs) = theirs else { continue };
            let (theirs, dx, dy) = line_up(&today, &theirs);
            let (dx, dy) = refine(&today, &theirs, (x, y, w, h), (dx, dy));
            eprintln!(
                "{label}: {}x{} offset {dx},{dy}",
                theirs.width(),
                theirs.height()
            );
            columns.push((
                label,
                theirs.crop_imm((x as i32 + dx) as u32, (y as i32 + dy) as u32, w, h),
            ));
        }
        let mut html = String::from(
            "<!doctype html><meta charset=utf-8><title>Against RAWmakase</title><style>\
             body{background:#1b1b1b;color:#ddd;font:14px system-ui;margin:16px}\
             .row{display:flex;flex-wrap:wrap;gap:8px}figure{margin:0}\
             img{width:720px;height:560px;image-rendering:pixelated;display:block}\
             figcaption{padding:4px 0}</style>",
        );
        html += &format!(
            "<h2>{} - 200%</h2><div class=row>",
            std::path::Path::new(&path)
                .file_name()
                .unwrap()
                .to_string_lossy()
        );
        for (i, (label, crop)) in columns.iter().enumerate() {
            let mut png = Vec::new();
            DynamicImage::ImageRgb8(crop.to_rgb8())
                .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
                .unwrap();
            let score = crispness(crop);
            eprintln!("{label}: crispness {score:.2}");
            html += &format!(
                "<figure><img src='data:image/png;base64,{}'><figcaption>{} - {label} (edge contrast {score:.1})</figcaption></figure>",
                base64::engine::general_purpose::STANDARD.encode(png),
                (b'A' + i as u8) as char,
            );
            DynamicImage::ImageRgb8(crop.to_rgb8())
                .save(out.join(format!("against_{i}.png")))
                .unwrap();
        }
        html += "</div>";
        std::fs::write(out.join("against-rawmakase.html"), html).unwrap();
    }
}
