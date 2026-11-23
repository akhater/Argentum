//! What the renderer is given for a stored edit, written as text: the
//! compatibility corpus's snapshot (compat_corpus.rs). Ours, test-only.
//!
//! For one edit it writes, with the code of today:
//!
//! - the `AllAdjustments` the GPU is sent (`get_all_adjustments_from_json`,
//!   as-shot white balance fixed at `AsShot::reference`, default
//!   `RenderOptions`, no photo), once as a RAW and once not: each 32-bit word
//!   that is not zero, named by its place in the struct;
//! - the geometry the warp takes (`get_geometry_params_from_json`), the size
//!   and crop offset `apply_spatial_transformations` gives a photo of
//!   `PHOTO_PX`, and the bitmap each mask makes at `MASK_SCALE` of it, as the
//!   mean of each `CELL_PX` square and of the whole.
//!
//! Not in it yet: the tools that read their values from the edit inside their
//! own pass rather than through `AllAdjustments` (fog, lens blur, relight, AI
//! denoise, the LUT). A renamed key there still passes this snapshot.
//!
//! Numbers are written to six significant digits and compared with a
//! tolerance (`TOLERANCE`, `MASK_TOLERANCE`): sines, cosines and powers come
//! from the platform's maths library, and Windows' and Linux's can differ in
//! the last bit, which an exact compare would fail on.
//!
//! The names come from the structs' own serde field order, which for these
//! `#[repr(C)]` structs of 4-byte fields is their memory order; `gpu_words`
//! checks every name against the word under it, so a field that is not four
//! bytes, or padding the compiler added, stops the test rather than
//! mislabelling the words after it.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use image::{DynamicImage, GenericImageView, GrayImage, Rgb, RgbImage};
use serde::de::{self, DeserializeSeed, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::Value;

use crate::as_shot::AsShot;
use crate::image_processing::{
    AllAdjustments, RenderOptions, get_all_adjustments_from_json, get_geometry_params_from_json,
    is_geometry_identity,
};
use crate::mask_generation::{MaskDefinition, build_full_warped_image, generate_mask_bitmap};

/// The photo every fixture's coordinates are in, before its orientation.
pub const PHOTO_PX: (u32, u32) = (600, 400);

/// Masks are drawn at this fraction of the photo, as a preview draws them.
pub const MASK_SCALE: f32 = 0.2;

/// A mask's bitmap is written as the mean of squares this many pixels a side.
const CELL_PX: u32 = 10;

/// How far a number may move before it counts as moved: absolute, then
/// relative to the larger of the two. Six significant digits, plus the last
/// bit of an f32 on another platform.
const TOLERANCE: (f64, f64) = (1e-6, 2e-5);

/// The same for a mask's means, in 8-bit levels: a pixel at an edge can come
/// out one level apart on another platform.
const MASK_TOLERANCE: f64 = 0.5;

/// The words after the photo's and the masks' in `AllAdjustments`.
const ALL_ADJUSTMENTS_TAIL: [&str; 4] = [
    "mask_count",
    "tile_offset_x",
    "tile_offset_y",
    "mask_atlas_cols",
];

/// One `[section]` of a snapshot: its lines, `name = value`, in order.
pub struct Section {
    pub title: String,
    pub lines: Vec<(String, String)>,
}

impl Section {
    fn new(title: String) -> Self {
        Self {
            title,
            lines: Vec::new(),
        }
    }

    fn push(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.lines.push((name.into(), value.into()));
    }
}

/// Every section for one edit, its titles starting with `label`.
pub fn sections_for(label: &str, edit: &Value) -> Vec<Section> {
    let mut out = Vec::new();
    for (is_raw, kind) in [(true, "raw"), (false, "not raw")] {
        let all = get_all_adjustments_from_json(
            edit,
            is_raw,
            AsShot::reference(),
            RenderOptions::default(),
            None,
        );
        let mut section = Section::new(format!("{label} / gpu, {kind}"));
        for (name, leaf) in gpu_words(&all) {
            if !leaf.is_zero() {
                section.push(name, leaf.to_string());
            }
        }
        out.push(section);
    }
    out.push(geometry(label, edit));

    let photo = synthetic_photo();
    let (transformed, crop_offset) =
        crate::adjustment_utils::apply_spatial_transformations(&photo, edit);
    let (width, height) = transformed.dimensions();
    let mut transform = Section::new(format!("{label} / transform"));
    transform.push("size_px", format!("{width} {height}"));
    transform.push(
        "crop_offset_px",
        format!("{} {}", sig6(crop_offset.0), sig6(crop_offset.1)),
    );
    out.push(transform);

    // As the render asks for them (file_management.rs, the thumbnail's
    // `get_cached_or_generate_mask`): the cropped size and the crop's corner,
    // both at the preview's scale, and the warped photo for colour and
    // luminance masks. A list that does not read gives no masks at all, as
    // it does there.
    let definitions: Vec<MaskDefinition> = edit
        .get("masks")
        .and_then(|m| serde_json::from_value(m.clone()).ok())
        .unwrap_or_default();
    let mut masks = Section::new(format!("{label} / masks"));
    masks.push("read", definitions.len().to_string());
    out.push(masks);
    let size = (
        (width as f32 * MASK_SCALE).round() as u32,
        (height as f32 * MASK_SCALE).round() as u32,
    );
    let offset = (crop_offset.0 * MASK_SCALE, crop_offset.1 * MASK_SCALE);
    let warped = build_full_warped_image(&photo, false, edit);
    for (index, definition) in definitions.iter().enumerate() {
        let Some(bitmap) = generate_mask_bitmap(
            definition,
            size.0,
            size.1,
            MASK_SCALE,
            offset,
            Some(warped.as_ref()),
        ) else {
            continue;
        };
        let mut section = Section::new(format!(
            "{label} / mask {index} {:?} {}",
            definition.name,
            definition
                .sub_masks
                .iter()
                .map(|s| s.mask_type.as_str())
                .collect::<Vec<_>>()
                .join("+")
        ));
        summarise(&bitmap, &mut section);
        out.push(section);
    }
    out
}

/// Every word of `all`, named.
fn gpu_words(all: &AllAdjustments) -> Vec<(String, Leaf)> {
    let words: &[u32] = bytemuck::cast_slice(std::slice::from_ref(all));
    let mut named = leaves_of("global", &all.global);
    for (i, mask) in all.mask_adjustments.iter().enumerate() {
        named.extend(leaves_of(&format!("mask[{i}]"), mask));
    }
    assert_eq!(
        named.len() + ALL_ADJUSTMENTS_TAIL.len(),
        words.len(),
        "AllAdjustments is {} words and its names cover {}: a field that is not 4 bytes, \
         or a new field after the masks (add it to ALL_ADJUSTMENTS_TAIL)",
        words.len(),
        named.len() + ALL_ADJUSTMENTS_TAIL.len()
    );
    let mut out: Vec<(String, Leaf)> = named
        .into_iter()
        .zip(words)
        .enumerate()
        .map(|(at, ((name, leaf), &word))| {
            let lined_up = match leaf {
                Leaf::Float(f) => (f as f32).to_bits() == word,
                Leaf::Int(i) => i == u64::from(word),
                // An f32 that is not finite: serde_json writes it as null.
                Leaf::Null => !f32::from_bits(word).is_finite(),
                Leaf::Bool(_) | Leaf::Text(_) => false,
            };
            assert!(
                lined_up,
                "{name} is not the word at {at}: the names no longer line up with the bytes"
            );
            let read = match leaf {
                Leaf::Int(_) => Leaf::Int(u64::from(word)),
                _ => Leaf::Float(f64::from(f32::from_bits(word))),
            };
            (name, read)
        })
        .collect();
    let tail = &words[words.len() - ALL_ADJUSTMENTS_TAIL.len()..];
    out.extend(
        ALL_ADJUSTMENTS_TAIL
            .iter()
            .zip(tail)
            .map(|(name, &word)| (name.to_string(), Leaf::Int(u64::from(word)))),
    );
    out
}

fn geometry(label: &str, edit: &Value) -> Section {
    let mut section = Section::new(format!("{label} / geometry"));
    let params = get_geometry_params_from_json(edit);
    section.push("identity", is_geometry_identity(&params).to_string());
    for (name, leaf) in leaves_of("", &params) {
        section.push(name, leaf.to_string());
    }
    match &params.lens_builtin {
        None => section.push("lens_builtin", "none"),
        Some(builtin) => {
            let amounts = builtin.amounts;
            section.push(
                "lens_builtin.amounts",
                format!(
                    "{} {} {}",
                    sig6(amounts.distortion),
                    sig6(amounts.chromatic),
                    sig6(amounts.vignetting)
                ),
            );
            for (name, leaf) in leaves_of("lens_builtin", builtin.correction.as_ref()) {
                section.push(name, leaf.to_string());
            }
        }
    }
    section
}

/// A photo with something in every channel: red across, green down, blue in
/// a checkerboard, so colour and luminance masks find a range to pick from.
fn synthetic_photo() -> DynamicImage {
    let (w, h) = PHOTO_PX;
    DynamicImage::ImageRgb8(RgbImage::from_fn(w, h, |x, y| {
        Rgb([
            (x * 255 / (w - 1)) as u8,
            (y * 255 / (h - 1)) as u8,
            if (x / 40 + y / 40) % 2 == 0 { 40 } else { 210 },
        ])
    }))
}

fn summarise(bitmap: &GrayImage, section: &mut Section) {
    let (w, h) = bitmap.dimensions();
    section.push("size_px", format!("{w} {h}"));
    let all: f64 = bitmap.pixels().map(|p| p[0] as f64).sum();
    section.push("mean", format!("{:.2}", all / (w * h).max(1) as f64));
    for row in 0..h.div_ceil(CELL_PX) {
        let mut cells = Vec::new();
        for col in 0..w.div_ceil(CELL_PX) {
            let (x0, y0) = (col * CELL_PX, row * CELL_PX);
            let (x1, y1) = ((x0 + CELL_PX).min(w), (y0 + CELL_PX).min(h));
            let mut sum = 0.0;
            for y in y0..y1 {
                for x in x0..x1 {
                    sum += bitmap.get_pixel(x, y)[0] as f64;
                }
            }
            cells.push(format!("{:.1}", sum / ((x1 - x0) * (y1 - y0)) as f64));
        }
        section.push(format!("row {row:02}"), cells.join(" "));
    }
}

/// The whole snapshot file for a fixture.
pub fn render(header: &str, sections: &[Section]) -> String {
    let mut out = String::new();
    for line in header.lines() {
        let _ = writeln!(out, "# {line}");
    }
    for section in sections {
        let _ = writeln!(out, "\n[{}]", section.title);
        for (name, value) in &section.lines {
            let _ = writeln!(out, "{name} = {value}");
        }
    }
    out
}

/// A snapshot file read back: each value under its section and name.
fn parse(text: &str) -> BTreeMap<(String, String), String> {
    let mut out = BTreeMap::new();
    let mut section = String::new();
    for line in text.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        if let Some(title) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = title.to_string();
        } else if let Some((name, value)) = line.split_once(" = ") {
            out.insert((section.clone(), name.to_string()), value.to_string());
        }
    }
    out
}

/// Every value that moved between the committed snapshot and today's, one
/// line each. A value written in only one of them counts as zero in the
/// other, since words that are zero are left out.
pub fn differences(committed: &str, today: &str) -> Vec<String> {
    let (was, now) = (parse(committed), parse(today));
    let mut keys: Vec<&(String, String)> = was.keys().chain(now.keys()).collect();
    keys.sort();
    keys.dedup();
    let mut out = Vec::new();
    for key in keys {
        let tolerance = if key.0.contains(" / mask ") {
            (MASK_TOLERANCE, 0.0)
        } else {
            TOLERANCE
        };
        let (a, b) = (was.get(key), now.get(key));
        let same = match (a, b) {
            (Some(a), Some(b)) => same_value(a, b, tolerance),
            (Some(only), None) | (None, Some(only)) => {
                numbers(only).is_some_and(|n| n.iter().all(|v| v.abs() <= tolerance.0))
            }
            (None, None) => true,
        };
        if !same {
            let show = |v: Option<&String>| v.map_or("(none)".to_string(), |v| v.clone());
            out.push(format!(
                "[{}] {}: was {}, now {}",
                key.0,
                key.1,
                show(a),
                show(b)
            ));
        }
    }
    out
}

/// The numbers in a value, or `None` when it is not all numbers.
fn numbers(value: &str) -> Option<Vec<f64>> {
    value
        .split_whitespace()
        .map(str::parse::<f64>)
        .collect::<Result<Vec<_>, _>>()
        .ok()
}

fn same_value(a: &str, b: &str, (absolute, relative): (f64, f64)) -> bool {
    match (numbers(a), numbers(b)) {
        (Some(x), Some(y)) => {
            x.len() == y.len()
                && x.iter()
                    .zip(&y)
                    .all(|(x, y)| (x - y).abs() <= absolute + relative * x.abs().max(y.abs()))
        }
        _ => a == b,
    }
}

/// A number to six significant digits, written as short as it reads back.
fn sig6(v: impl Into<f64>) -> String {
    let v: f64 = v.into();
    if v == 0.0 {
        return "0".to_string();
    }
    if !v.is_finite() {
        return v.to_string();
    }
    format!("{:.5e}", v)
        .parse::<f64>()
        .map_or_else(|_| v.to_string(), |rounded| rounded.to_string())
}

/// A value at the end of a path through a struct.
#[derive(Clone, Debug, PartialEq)]
enum Leaf {
    Float(f64),
    Int(u64),
    Bool(bool),
    Text(String),
    /// What serde_json writes for an f32 that is not finite.
    Null,
}

impl Leaf {
    fn is_zero(&self) -> bool {
        match self {
            Leaf::Float(f) => *f == 0.0,
            Leaf::Int(i) => *i == 0,
            _ => false,
        }
    }
}

impl std::fmt::Display for Leaf {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Leaf::Float(v) => f.write_str(&sig6(*v)),
            Leaf::Int(v) => write!(f, "{v}"),
            Leaf::Bool(v) => write!(f, "{v}"),
            Leaf::Text(v) => write!(f, "{v:?}"),
            Leaf::Null => f.write_str("null"),
        }
    }
}

/// Every value in `value`, named by its path (`global.hsl[3].saturation`),
/// in the order serde writes them: a struct's field order. Read back from
/// serde_json's text because its `Value` keeps no order.
fn leaves_of<T: serde::Serialize + ?Sized>(prefix: &str, value: &T) -> Vec<(String, Leaf)> {
    let text = serde_json::to_string(value).expect("serialises");
    let mut out = Vec::new();
    let mut reader = serde_json::Deserializer::from_str(&text);
    Flatten {
        name: prefix.to_string(),
        out: &mut out,
    }
    .deserialize(&mut reader)
    .expect("reads back");
    out
}

struct Flatten<'a> {
    name: String,
    out: &'a mut Vec<(String, Leaf)>,
}

impl<'de> DeserializeSeed<'de> for Flatten<'_> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Flatten<'_> {
    type Value = ();

    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("any JSON")
    }

    fn visit_bool<E: de::Error>(self, v: bool) -> Result<(), E> {
        self.out.push((self.name, Leaf::Bool(v)));
        Ok(())
    }

    fn visit_u64<E: de::Error>(self, v: u64) -> Result<(), E> {
        self.out.push((self.name, Leaf::Int(v)));
        Ok(())
    }

    fn visit_i64<E: de::Error>(self, v: i64) -> Result<(), E> {
        self.out.push((self.name, Leaf::Float(v as f64)));
        Ok(())
    }

    fn visit_f64<E: de::Error>(self, v: f64) -> Result<(), E> {
        self.out.push((self.name, Leaf::Float(v)));
        Ok(())
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<(), E> {
        self.out.push((self.name, Leaf::Text(v.to_string())));
        Ok(())
    }

    fn visit_unit<E: de::Error>(self) -> Result<(), E> {
        self.out.push((self.name, Leaf::Null));
        Ok(())
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        let out = self.out;
        let mut index = 0;
        while seq
            .next_element_seed(Flatten {
                name: format!("{}[{index}]", self.name),
                out: &mut *out,
            })?
            .is_some()
        {
            index += 1;
        }
        Ok(())
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        let out = self.out;
        while let Some(key) = map.next_key::<String>()? {
            let name = if self.name.is_empty() {
                key
            } else {
                format!("{}.{key}", self.name)
            };
            map.next_value_seed(Flatten {
                name,
                out: &mut *out,
            })?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_number_is_written_to_six_significant_digits() {
        assert_eq!(sig6(0.312_500_01_f64), "0.3125");
        assert_eq!(sig6(1_234_567.0_f64), "1234570");
        assert_eq!(sig6(-0.000_012_345_678_f64), "-0.0000123457");
        assert_eq!(sig6(0.0_f32), "0");
    }

    #[test]
    fn a_value_moves_only_past_the_tolerance() {
        let was = "[a / gpu, raw]\nglobal.exposure = 0.3125\nglobal.contrast = 0.18\n";
        assert!(differences(was, was).is_empty());
        // The last digit on another platform.
        let near = "[a / gpu, raw]\nglobal.exposure = 0.312501\nglobal.contrast = 0.18\n";
        assert!(differences(was, near).is_empty());
        // Gone to zero, so left out.
        let zeroed = "[a / gpu, raw]\nglobal.exposure = 0.3125\n";
        assert_eq!(
            differences(was, zeroed),
            ["[a / gpu, raw] global.contrast: was 0.18, now (none)"]
        );
        // Almost zero on one platform, zero on the other.
        assert!(
            differences(
                zeroed,
                "[a / gpu, raw]\nglobal.exposure = 0.3125\nglobal.contrast = 1e-9\n"
            )
            .is_empty()
        );
    }

    #[test]
    fn a_field_added_since_the_snapshot_passes_at_zero_and_fails_otherwise() {
        let was = "[a / geometry]\nlens_dist_k1 = 0\n";
        // A field the printer learned later, resolving to zero: the render
        // is what it was, so the older snapshot stays valid.
        let added_zero = "[a / geometry]\nlens_dist_k1 = 0\ntca_cr = 0\ntca_bb = 0\n";
        assert!(differences(was, added_zero).is_empty());
        // The same field resolving to a value: an old edit now renders
        // differently, which must stop the test.
        let added_value = "[a / geometry]\nlens_dist_k1 = 0\ntca_cr = 0.002\n";
        assert_eq!(
            differences(was, added_value),
            ["[a / geometry] tca_cr: was (none), now 0.002"]
        );
    }

    #[test]
    fn a_mask_cell_may_move_half_a_level() {
        let was = "[a / mask 0 \"M\" radial]\nrow 00 = 10.0 20.0\n";
        assert!(differences(was, "[a / mask 0 \"M\" radial]\nrow 00 = 10.4 20.0\n").is_empty());
        assert_eq!(
            differences(was, "[a / mask 0 \"M\" radial]\nrow 00 = 12.0 20.0\n").len(),
            1
        );
    }

    #[test]
    fn every_word_of_the_gpu_input_has_a_name() {
        let all = get_all_adjustments_from_json(
            &serde_json::json!({ "exposure": 1.0, "masks": [] }),
            true,
            AsShot::reference(),
            RenderOptions::default(),
            None,
        );
        let words = gpu_words(&all);
        assert!(words.iter().any(|(name, _)| name == "global.exposure"));
        assert!(
            words
                .iter()
                .any(|(name, _)| name == "mask[31].blue_curve_count")
        );
        assert_eq!(
            words.last().map(|(name, _)| name.as_str()),
            Some("mask_atlas_cols")
        );
    }
}
