//! Lightroom's colour grading in today's app. Ours.
//!
//! WHAT IT IS
//!
//! RapidRAW's Color Grading (`apply_color_grading` in shader.wgsl) adds a
//! constant tint scaled down to a tenth in the shadows, so it shows in the
//! darkest tones and hardly anywhere else: Lightroom's split toning at
//! Saturation 25 barely shows on a mid-grey. RAWmakase measured how Camera
//! Raw grades (the engine's `rm_color_grade.rs`). The same wheels and sliders,
//! in the same units, render with that.
//!
//! WHICH EDITS
//!
//! The new maths changes how an existing grading looks, so a Color Grading
//! block graded before it keeps RapidRAW's. A block - the photo's, or a
//! mask's - is Lightroom's when it carries `"agLightroom": 1`: the panel
//! writes it the first time a block that grades nothing is changed
//! (src/argentum/colorGrading.ts), and the Looks carry it (`mark_lightroom`,
//! from mods/looks.rs). A block without it that grades something was saved
//! before, and renders as it did. The mark lives inside the block, so Copy,
//! Paste and presets carry it with the grading.
//!
//! WHERE
//!
//! Where Camera Raw grades: after the tone curve, the Curves panel and a
//! LUT, on display-referred linear values (`ag_stage_grade` in modules.wgsl).
//! RapidRAW's grading runs earlier, on scene-linear values. Each render,
//! `move_to_curves` takes a Lightroom block's settings out of RapidRAW's
//! uniforms, so their grading adds nothing for it, and turns them into the
//! 64 gains per channel the GPU looks up.

use argentum_engine::rm_color_grade::{self, Gains, SAMPLES};
use bytemuck::{Pod, Zeroable};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::image_processing::{AllAdjustments, ColorGradeSettings, MAX_MASKS};

/// The key inside a `colorGrading` block that marks it as Lightroom's, with
/// the value 1. A number, as their block's type holds only numbers and wheels.
pub const LIGHTROOM_KEY: &str = "agLightroom";

/// Slots in the curves buffer: the photo's block, then each mask's, as their
/// shader indexes masks.
pub const SLOTS: usize = 1 + MAX_MASKS;
/// f32 values per slot: `SAMPLES` gains for each of three channels, sample
/// by sample.
pub const SLOT_LEN: usize = SAMPLES * 3;

/// Which blocks are Lightroom's, in the layout the GPU struct carries
/// (`AgGrade` in modules.wgsl). It rides inside RapidRAW's
/// `GlobalAdjustments`, as `sharpen::Params` does. 16 bytes.
#[repr(C)]
#[derive(Serialize, Deserialize, Debug, Clone, Copy, Pod, Zeroable, Default, PartialEq)]
pub struct Params {
    /// 1 when the photo's own block is Lightroom's.
    pub global: u32,
    /// Bit i: mask i's block.
    pub masks: u32,
    _pad: [u32; 2],
}

impl Params {
    /// The photo's own block, from its adjustments; no mask marked yet.
    pub fn of(adjustments: &Value) -> Self {
        Self {
            global: u32::from(is_lightroom(adjustments)),
            ..Self::default()
        }
    }

    /// Marks mask `index` when its adjustments' block is Lightroom's.
    pub fn mark_mask(&mut self, index: usize, mask_adjustments: &Value) {
        if index < MAX_MASKS && is_lightroom(mask_adjustments) {
            self.masks |= 1 << index;
        }
    }

    pub fn is_on(&self) -> bool {
        self.global != 0 || self.masks != 0
    }
}

/// Whether these adjustments' `colorGrading` block is Lightroom's.
fn is_lightroom(adjustments: &Value) -> bool {
    adjustments
        .get("colorGrading")
        .and_then(|block| block.get(LIGHTROOM_KEY))
        .and_then(Value::as_f64)
        == Some(1.0)
}

/// A preset's adjustments with their `colorGrading` block, if they have one,
/// marked as Lightroom's: for presets read from Lightroom's own XMP.
pub fn mark_lightroom(mut adjustments: Value) -> Value {
    if let Some(Value::Object(block)) = adjustments.get_mut("colorGrading") {
        block.insert(LIGHTROOM_KEY.to_string(), Value::from(1));
    }
    adjustments
}

/// One block's settings, as their uniforms hold them.
struct Block<'a> {
    regions: [&'a mut ColorGradeSettings; 4],
    blending: f32,
    balance: f32,
}

impl Block<'_> {
    /// The block's gains, its settings taken out of the uniforms so their
    /// grading adds nothing. `None` when it grades nothing.
    fn take(self) -> Option<Gains> {
        let settings = self.regions.each_ref().map(|s| **s);
        let gains = rm_color_grade::gains(&crate::image_processing::lightroom_grading(
            settings,
            self.blending,
            self.balance,
        ));
        for region in self.regions {
            *region = ColorGradeSettings::default();
        }
        gains
    }
}

/// For one render: the Lightroom blocks' curves, a slot each (no change in
/// the others), with their settings moved out of RapidRAW's uniforms. `None`
/// when no block is Lightroom's.
pub fn move_to_curves(adjustments: &mut AllAdjustments) -> Option<Vec<f32>> {
    let marked = adjustments.global.ag_grade;
    if !marked.is_on() {
        return None;
    }
    let mut curves = vec![1.0; SLOTS * SLOT_LEN];
    let mut write = |slot: usize, gains: Option<Gains>| {
        if let Some(gains) = gains {
            curves[slot * SLOT_LEN..(slot + 1) * SLOT_LEN].copy_from_slice(gains.as_flattened());
        }
    };
    if marked.global != 0 {
        let g = &mut adjustments.global;
        let block = Block {
            blending: g.color_grading_blending,
            balance: g.color_grading_balance,
            regions: [
                &mut g.color_grading_shadows,
                &mut g.color_grading_midtones,
                &mut g.color_grading_highlights,
                &mut g.color_grading_global,
            ],
        };
        write(0, block.take());
    }
    let count = (adjustments.mask_count as usize).min(MAX_MASKS);
    for (i, m) in adjustments.mask_adjustments[..count].iter_mut().enumerate() {
        if marked.masks & (1 << i) == 0 {
            continue;
        }
        let block = Block {
            blending: m.color_grading_blending,
            balance: m.color_grading_balance,
            regions: [
                &mut m.color_grading_shadows,
                &mut m.color_grading_midtones,
                &mut m.color_grading_highlights,
                &mut m.color_grading_global,
            ],
        };
        write(1 + i, block.take());
    }
    Some(curves)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image_processing::{RenderOptions, get_all_adjustments_from_json};
    use argentum_engine::rm_color_grade::Grading;
    use serde_json::json;

    fn render(adjustments: &Value) -> AllAdjustments {
        get_all_adjustments_from_json(
            adjustments,
            true,
            crate::as_shot::AsShot::reference(),
            RenderOptions::default(),
            None,
        )
    }

    fn sepia(marked: bool) -> Value {
        let mut block = json!({
            "shadows": { "hue": 38, "saturation": 25, "luminance": 0 },
            "highlights": { "hue": 45, "saturation": 18, "luminance": 0 },
            "blending": 50,
            "balance": 0,
        });
        if marked {
            block[LIGHTROOM_KEY] = json!(1);
        }
        json!({ "colorGrading": block })
    }

    #[test]
    fn an_unmarked_block_keeps_rapidraws_grading() {
        let mut all = render(&sepia(false));
        assert_eq!(all.global.ag_grade, Params::default());
        assert!(move_to_curves(&mut all).is_none());
        // Their uniforms are untouched: their grading still runs.
        assert!(all.global.color_grading_shadows.saturation > 0.0);
    }

    #[test]
    fn a_marked_block_moves_to_lightroom_s_curves() {
        let mut all = render(&sepia(true));
        assert_eq!(all.global.ag_grade.global, 1);
        let curves = move_to_curves(&mut all).expect("curves");
        assert_eq!(curves.len(), SLOTS * SLOT_LEN);
        // Their grading adds nothing now.
        assert_eq!(all.global.color_grading_shadows.saturation, 0.0);
        assert_eq!(all.global.color_grading_highlights.saturation, 0.0);
        // The same gains as the engine gives Sepia in Lightroom's units.
        let expected = rm_color_grade::gains(&Grading {
            regions: [
                [38. / 360., 0.25, 0.],
                [0.; 3],
                [45. / 360., 0.18, 0.],
                [0.; 3],
            ],
            ..Grading::default()
        })
        .unwrap();
        for (got, want) in curves[..SLOT_LEN].iter().zip(expected.as_flattened()) {
            assert!((got - want).abs() < 1e-5, "{got} {want}");
        }
        // Masks' slots change nothing.
        assert!(curves[SLOT_LEN..].iter().all(|&g| g == 1.0));
    }

    #[test]
    fn masks_are_marked_in_their_shader_order() {
        let mask = |marked: bool, visible: bool| {
            json!({
                "id": "m",
                "name": "m",
                "visible": visible,
                "invert": false,
                "subMasks": [{
                    "id": "s",
                    "type": "radial",
                    "visible": true,
                    "mode": "additive",
                    "parameters": {},
                }],
                "adjustments": sepia(marked),
            })
        };
        let adjustments = json!({
            "masks": [mask(true, false), mask(false, true), mask(true, true)],
        });
        let mut all = render(&adjustments);
        // The hidden mask is not sent to the shader, so the third is mask 1.
        assert_eq!(all.mask_count, 2);
        assert_eq!(all.global.ag_grade.masks, 0b10);
        let curves = move_to_curves(&mut all).expect("curves");
        assert!(curves[..2 * SLOT_LEN].iter().all(|&g| g == 1.0));
        assert!(curves[2 * SLOT_LEN..3 * SLOT_LEN].iter().any(|&g| g != 1.0));
        assert!(all.mask_adjustments[0].color_grading_shadows.saturation > 0.0);
        assert_eq!(
            all.mask_adjustments[1].color_grading_shadows.saturation,
            0.0
        );
    }

    #[test]
    fn looks_are_marked_and_presets_without_grading_are_left_alone() {
        let marked = mark_lightroom(sepia(false));
        assert_eq!(marked["colorGrading"][LIGHTROOM_KEY], 1);
        let plain = json!({ "exposure": 0.5 });
        assert_eq!(mark_lightroom(plain.clone()), plain);
    }
}

/// Through the real shader, on a GPU: `cargo test --lib color_grade::gpu -- --ignored`.
#[cfg(test)]
mod gpu {
    use super::*;
    use crate::mods::dng_standard::gpu::{centre, device, image_of, render};
    use crate::white_balance::WhiteBalance;
    use argentum_engine::colour::{srgb_decode, srgb_encode};
    use serde_json::json;

    // modules.wgsl's AG_SRGB_TO_PROPHOTO and AG_PROPHOTO_TO_SRGB, row by row.
    const SRGB_TO_PROPHOTO: [[f32; 3]; 3] = [
        [0.5293458, 0.3300728, 0.1405813],
        [0.0983744, 0.873461, 0.0281647],
        [0.0168832, 0.1176725, 0.8654443],
    ];
    const PROPHOTO_TO_SRGB: [[f32; 3]; 3] = [
        [2.0340764, -0.7273343, -0.3067417],
        [-0.2288134, 1.2317302, -0.0029169],
        [-0.0085698, -0.1532866, 1.1618564],
    ];

    fn mul(m: [[f32; 3]; 3], c: [f32; 3]) -> [f32; 3] {
        m.map(|row| row[0] * c[0] + row[1] * c[1] + row[2] * c[2])
    }

    /// Greys, and colours round the hue circle at three levels.
    fn colours() -> Vec<[f32; 3]> {
        let mut out: Vec<[f32; 3]> = [0.005, 0.02, 0.06, 0.18, 0.4, 0.8]
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

    fn edit(grading: Value) -> Value {
        json!({ "agSharpen": { "capture": false, "amount": 0 }, "colorGrading": grading })
    }

    fn marked(mut grading: Value) -> Value {
        grading[LIGHTROOM_KEY] = json!(1);
        grading
    }

    /// The shader grades as the engine does: a patch rendered with a
    /// Lightroom block is the patch rendered without grading, graded on the
    /// CPU by `rm_color_grade::apply` in linear ProPhoto.
    #[test]
    #[ignore = "needs a GPU; run with --ignored"]
    fn gpu_the_shader_grades_as_the_engine_does() {
        let ctx = device().expect("no wgpu adapter");
        let wb = WhiteBalance::reference();
        let image = image_of(&colours());
        let plain = render(&ctx, &image, edit(json!({})), wb);
        for grading in [
            json!({
                "shadows": { "hue": 38, "saturation": 25, "luminance": 0 },
                "highlights": { "hue": 45, "saturation": 18, "luminance": 0 },
                "blending": 50, "balance": 0,
            }),
            json!({
                "shadows": { "hue": 200, "saturation": 60, "luminance": -30 },
                "midtones": { "hue": 100, "saturation": 40, "luminance": 20 },
                "highlights": { "hue": 20, "saturation": 80, "luminance": 40 },
                "global": { "hue": 300, "saturation": 10, "luminance": -10 },
                "blending": 80, "balance": -35,
            }),
        ] {
            let all = crate::image_processing::get_all_adjustments_from_json(
                &edit(marked(grading.clone())),
                true,
                crate::as_shot::AsShot::reference(),
                Default::default(),
                None,
            );
            let g = &all.global;
            let gains = rm_color_grade::gains(&crate::image_processing::lightroom_grading(
                [
                    g.color_grading_shadows,
                    g.color_grading_midtones,
                    g.color_grading_highlights,
                    g.color_grading_global,
                ],
                g.color_grading_blending,
                g.color_grading_balance,
            ))
            .unwrap();
            let got = render(&ctx, &image, edit(marked(grading.clone())), wb);
            let mut worst = 0.0f32;
            for i in 0..colours().len() {
                let linear = centre(&plain, i).map(srgb_decode);
                let graded = mul(
                    PROPHOTO_TO_SRGB,
                    rm_color_grade::apply(&gains, mul(SRGB_TO_PROPHOTO, linear)),
                );
                let want = graded.map(srgb_encode);
                let have = centre(&got, i).map(|v| v.clamp(0.0, 1.0));
                for c in 0..3 {
                    worst = worst.max((have[c] - want[c]).abs() * 255.0);
                }
            }
            eprintln!("{grading}: worst {worst:.3}/255 from the engine's grading");
            assert!(worst < 0.5, "the shader is {worst:.3}/255 from the engine");
        }
    }

    /// A Lightroom block tints a mid-grey that RapidRAW's grading leaves
    /// almost untouched, and an unmarked block still renders with theirs.
    #[test]
    #[ignore = "needs a GPU; run with --ignored"]
    fn gpu_lightroom_s_sepia_reaches_the_mid_greys() {
        let ctx = device().expect("no wgpu adapter");
        let wb = WhiteBalance::reference();
        let image = image_of(&colours());
        let sepia = json!({
            "shadows": { "hue": 38, "saturation": 25, "luminance": 0 },
            "highlights": { "hue": 45, "saturation": 18, "luminance": 0 },
            "blending": 50, "balance": 0,
        });
        let plain = render(&ctx, &image, edit(json!({})), wb);
        let theirs = render(&ctx, &image, edit(sepia.clone()), wb);
        let ours = render(&ctx, &image, edit(marked(sepia)), wb);
        let warmth = |out: &image::Rgba32FImage, i: usize| {
            let c = centre(out, i);
            c[0] - c[2]
        };
        // Patch 3 is the 18% grey.
        let (plain, theirs, ours) = (warmth(&plain, 3), warmth(&theirs, 3), warmth(&ours, 3));
        eprintln!(
            "18% grey, red minus blue: none {plain:.4}, RapidRAW {theirs:.4}, Lightroom {ours:.4}"
        );
        assert!(
            theirs != plain,
            "the unmarked block lost RapidRAW's grading"
        );
        assert!(
            ours - plain > 0.03,
            "Lightroom's Sepia barely tints: {ours}"
        );
        assert!(ours - plain > 3.0 * (theirs - plain).abs());
    }
}

/// Renders for judging the grading by eye, on real photos: `AG_RAWS` (paths
/// separated by ';') and `AG_OUT` set, `cargo test --lib color_grade::sheet -- --ignored`.
#[cfg(test)]
mod sheet {
    use super::*;
    use crate::mods::dng_standard::gpu::device;
    use image::{DynamicImage, GenericImageView};

    /// The Looks that come with the app and grade.
    const GRADED: [&str; 5] = ["Cool", "Matte", "Warm", "Faded Gold", "Sepia"];

    fn save(image: &DynamicImage, path: &std::path::Path) {
        let file = std::fs::File::create(path).expect("create");
        let mut jpeg = image::codecs::jpeg::JpegEncoder::new_with_quality(file, 95);
        jpeg.encode_image(&DynamicImage::ImageRgb8(image.to_rgb8()))
            .expect("write jpeg");
    }

    /// The Look as RapidRAW grades it: the same settings without the mark.
    fn unmarked(mut adjustments: Value) -> Value {
        if let Some(Value::Object(block)) = adjustments.get_mut("colorGrading") {
            block.remove(LIGHTROOM_KEY);
        }
        adjustments
    }

    /// Each RAW developed as the app does, then rendered with each graded
    /// Look that comes with the app, as RapidRAW grades it and as Lightroom
    /// does: `<n>-<look>-rapidraw.jpg` and `<n>-<look>-lightroom.jpg`,
    /// `AG_WIDTH` wide (1200 by default), and where `AG_CROPS` gives the photo
    /// a part `x,y,w,h` (fractions, ';' per RAW), `...-crop.jpg` of that part
    /// at full size. A Look not in this build (Sepia before B&W) is skipped.
    #[test]
    #[ignore = "reads RAW files; run by hand"]
    fn sheet_renders() {
        let raws = std::env::var("AG_RAWS").expect("set AG_RAWS");
        let out = std::path::PathBuf::from(std::env::var("AG_OUT").expect("set AG_OUT"));
        let width: u32 = std::env::var("AG_WIDTH")
            .ok()
            .and_then(|w| w.parse().ok())
            .unwrap_or(1200);
        let crops: Vec<Option<[f32; 4]>> = std::env::var("AG_CROPS")
            .unwrap_or_default()
            .split(';')
            .map(|c| {
                let v: Vec<f32> = c.split(',').filter_map(|n| n.trim().parse().ok()).collect();
                (v.len() == 4).then(|| [v[0], v[1], v[2], v[3]])
            })
            .collect();
        std::fs::create_dir_all(&out).unwrap();
        let ctx = device().expect("no wgpu adapter");
        let shipped =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/presets/rawmakase");
        let mut looks: Vec<(String, Value)> = Vec::new();
        for group in crate::mods::looks::builtin_presets(&shipped) {
            for preset in group.presets {
                if GRADED.contains(&preset.name.as_str()) {
                    let name = preset.name.to_lowercase().replace(' ', "-");
                    looks.push((
                        format!("{name}-rapidraw"),
                        unmarked(preset.adjustments.clone()),
                    ));
                    looks.push((format!("{name}-lightroom"), preset.adjustments));
                }
            }
        }
        for (n, path) in raws.split(';').filter(|p| !p.trim().is_empty()).enumerate() {
            let bytes = std::fs::read(path).expect("read raw");
            let as_shot = crate::raw_processing::read_as_shot(&bytes)
                .unwrap_or_else(crate::as_shot::AsShot::reference);
            let linear = crate::raw_processing::develop_raw_image(
                &bytes,
                false,
                2.5,
                "default".to_string(),
                None,
                Some(path),
            )
            .expect("decodes");
            let crop = crops.get(n).copied().flatten();
            let source = if crop.is_some() {
                linear
            } else {
                let h = (linear.height() as f32 * width as f32 / linear.width() as f32) as u32;
                crate::image_processing::downscale_f32_image(&linear, width, h)
            };
            for (look, js) in &looks {
                let adjustments = crate::image_processing::get_all_adjustments_from_json(
                    js,
                    true,
                    as_shot,
                    Default::default(),
                    Some(path),
                );
                let rendered = crate::mods::export_precision::render_high_precision(
                    &ctx,
                    &source,
                    crate::gpu_processing::RenderRequest {
                        adjustments,
                        mask_bitmaps: &[],
                        lut: None,
                        roi: None,
                    },
                )
                .expect("renders");
                let name = format!("{}-{look}", n + 1);
                let (w, h) = rendered.dimensions();
                let whole =
                    rendered.resize(width, width * h / w, image::imageops::FilterType::Lanczos3);
                save(&whole, &out.join(format!("{name}.jpg")));
                if let Some([x, y, cw, ch]) = crop {
                    let part = rendered.crop_imm(
                        (x * w as f32) as u32,
                        (y * h as f32) as u32,
                        (cw * w as f32) as u32,
                        (ch * h as f32) as u32,
                    );
                    save(&part, &out.join(format!("{name}-crop.jpg")));
                }
                eprintln!("{path}: {look}");
            }
        }
    }
}
