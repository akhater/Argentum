use regex::Regex;
use regex::regex;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use uuid::Uuid;

use crate::file_management::Preset;
use crate::white_balance::{MIRED_PER_RELATIVE_UNIT, TINT_PER_RELATIVE_UNIT};

#[derive(Copy, Clone, Debug)]
enum Num {
    I(i64),
    F(f64),
}

fn parse_num(s: &str) -> Option<Num> {
    if let Ok(i) = s.parse::<i64>() {
        Some(Num::I(i))
    } else if let Ok(f) = s.parse::<f64>() {
        Some(Num::F(f))
    } else {
        None
    }
}

fn num_to_json(num: Num) -> Option<Value> {
    match num {
        Num::I(i) => Some(Value::Number(i.into())),
        Num::F(f) => serde_json::Number::from_f64(f).map(Value::Number),
    }
}

fn get_attr_as_f64(attrs: &HashMap<String, String>, key: &str) -> Option<f64> {
    attrs
        .get(key)
        .and_then(|s| s.trim_start_matches('+').parse::<f64>().ok())
}

fn extract_xmp_name(xmp_content: &str) -> Option<String> {
    regex!(r#"(?s)<crs:Name>.*?<rdf:Alt>.*?<rdf:li[^>]*>([^<]+)</rdf:li>.*?</crs:Name>"#)
        .captures(xmp_content)
        .and_then(|c| c.get(1).map(|m| m.as_str().trim().to_string()))
}

fn extract_tone_curve_points(xmp_str: &str, curve_name: &str) -> Option<Vec<Value>> {
    let pattern = format!(
        r"(?s)<crs:{}>\s*<rdf:Seq>(.*?)</rdf:Seq>\s*</crs:{}>",
        curve_name, curve_name
    );
    let re = Regex::new(&pattern).ok()?;
    let captures = re.captures(xmp_str)?;
    let seq_content = captures.get(1)?.as_str();

    let point_re = regex!(r"<rdf:li>(\d+),\s*(\d+)</rdf:li>");
    let mut points = Vec::new();

    for point_cap in point_re.captures_iter(seq_content) {
        let x: u32 = point_cap.get(1)?.as_str().parse().ok()?;
        let y: u32 = point_cap.get(2)?.as_str().parse().ok()?;

        let mut final_y = y;
        // The master curve, of the 2012 set or of the older one.
        if matches!(curve_name, "ToneCurvePV2012" | "ToneCurve") {
            const SHADOW_RANGE_END: f64 = 64.0;
            const SHADOW_DAMPEN_START: f64 = 0.8;
            const SHADOW_DAMPEN_END: f64 = 1.0;

            let x_f64 = x as f64;
            let y_f64 = y as f64;

            if y_f64 > x_f64 && x_f64 < SHADOW_RANGE_END {
                let lift_amount = y_f64 - x_f64;
                let progress = x_f64 / SHADOW_RANGE_END;
                let dampening_factor =
                    SHADOW_DAMPEN_START + (SHADOW_DAMPEN_END - SHADOW_DAMPEN_START) * progress;

                let new_y = x_f64 + (lift_amount * dampening_factor);
                final_y = new_y.round().clamp(0.0, 255.0) as u32;
            }
        }

        let mut point = Map::new();
        point.insert("x".to_string(), Value::Number(x.into()));
        point.insert("y".to_string(), Value::Number(final_y.into()));
        points.push(Value::Object(point));
    }

    if points.is_empty() {
        None
    } else {
        Some(points)
    }
}

pub fn convert_xmp_to_preset(xmp_content: &str) -> Result<Preset, String> {
    let xmp_one_line = xmp_content.split('\n').collect::<Vec<_>>().join(" ");

    let attr_re = regex!(r#"crs:([A-Za-z0-9]+)="([^"]*)""#);
    let mut attrs: HashMap<String, String> = HashMap::new();
    for cap in attr_re.captures_iter(&xmp_one_line) {
        attrs.insert(cap[1].to_string(), cap[2].to_string());
    }
    // Lightroom 3 and 4 presets (process versions 2003/2010) carry only the
    // old basic controls: convert them into the 2012 keys read below.
    crate::mods::lr_legacy::apply_legacy_tone(&mut attrs);

    let mut adjustments = Map::new();
    let mut hsl_map = Map::new();
    let mut color_grading_map = Map::new();
    let mut curves_map = Map::new();

    let mappings = vec![
        ("Exposure2012", "exposure"),
        ("Contrast2012", "contrast"),
        ("Highlights2012", "highlights"),
        ("Whites2012", "whites"),
        ("Blacks2012", "blacks"),
        ("Clarity2012", "clarity"),
        ("Dehaze", "dehaze"),
        ("Vibrance", "vibrance"),
        ("Saturation", "saturation"),
        ("Texture", "structure"),
        ("SharpenRadius", "sharpenRadius"),
        ("SharpenDetail", "sharpenDetail"),
        ("SharpenEdgeMasking", "sharpenMasking"),
        ("LuminanceSmoothing", "lumaNoiseReduction"),
        ("ColorNoiseReduction", "colorNoiseReduction"),
        ("ColorNoiseReductionDetail", "colorNoiseDetail"),
        ("ColorNoiseReductionSmoothness", "colorNoiseSmoothness"),
        ("ChromaticAberrationRedCyan", "chromaticAberrationRedCyan"),
        (
            "ChromaticAberrationBlueYellow",
            "chromaticAberrationBlueYellow",
        ),
        ("PostCropVignetteAmount", "vignetteAmount"),
        ("PostCropVignetteMidpoint", "vignetteMidpoint"),
        ("PostCropVignetteFeather", "vignetteFeather"),
        ("PostCropVignetteRoundness", "vignetteRoundness"),
        ("GrainAmount", "grainAmount"),
        ("GrainSize", "grainSize"),
        ("GrainFrequency", "grainRoughness"),
        ("ColorGradeBlending", "blending"),
    ];

    for (xmp_key, rr_key) in mappings {
        if let Some(raw_val) = attrs.get(xmp_key)
            && let Some(num) = parse_num(raw_val.trim_start_matches('+'))
            && let Some(json_val) = num_to_json(num)
        {
            if rr_key == "blending" {
                color_grading_map.insert(rr_key.to_string(), json_val);
            } else {
                adjustments.insert(rr_key.to_string(), json_val);
            }
        }
    }

    if let Some(shadows_val) = get_attr_as_f64(&attrs, "Shadows2012") {
        // RapidRAW 61960fc5: src-tauri/src/library/preset_converter.rs
        // Clamp both ends: Shadows -100 scaled by 1.5 must stay -100, not -150.
        let adjusted_shadows = (shadows_val * 1.5).clamp(-100.0, 100.0);
        adjustments.insert("shadows".to_string(), json!(adjusted_shadows));
    }

    if let Some(sharpness_val) = get_attr_as_f64(&attrs, "Sharpness") {
        let scaled_sharpness = (sharpness_val / 150.0) * 100.0;
        adjustments.insert(
            "sharpness".to_string(),
            json!(scaled_sharpness.clamp(0.0, 100.0)),
        );
    }

    if let Some(adjusted_k) = get_attr_as_f64(&attrs, "Temperature") {
        const AS_SHOT_DEFAULT: f64 = 5500.0;
        let as_shot_k = get_attr_as_f64(&attrs, "AsShotTemperature").unwrap_or(AS_SHOT_DEFAULT);
        let mired_adjusted = 1_000_000.0 / adjusted_k;
        let mired_as_shot = 1_000_000.0 / as_shot_k;
        let mired_delta = mired_adjusted - mired_as_shot;
        let temp_value = -mired_delta / MIRED_PER_RELATIVE_UNIT;
        adjustments.insert(
            "temperature".to_string(),
            json!(temp_value.clamp(-100.0, 100.0)),
        );
    }

    if let Some(tint_val) = get_attr_as_f64(&attrs, "Tint") {
        let scaled_tint = tint_val / TINT_PER_RELATIVE_UNIT;
        adjustments.insert("tint".to_string(), json!(scaled_tint.clamp(-100.0, 100.0)));
    }

    let colors = [
        ("Red", "reds"),
        ("Orange", "oranges"),
        ("Yellow", "yellows"),
        ("Green", "greens"),
        ("Aqua", "aquas"),
        ("Blue", "blues"),
        ("Purple", "purples"),
        ("Magenta", "magentas"),
    ];
    for (src, dst) in colors {
        let mut color_map = Map::new();
        if let Some(raw) = attrs.get(&format!("HueAdjustment{}", src))
            && let Some(num) = parse_num(raw.trim_start_matches('+'))
            && let Some(Value::Number(n)) = num_to_json(num)
            && let Some(val_f64) = n.as_f64()
        {
            let adjusted_hue = val_f64 * 0.75;
            color_map.insert("hue".to_string(), json!(adjusted_hue));
        }
        if let Some(raw) = attrs.get(&format!("SaturationAdjustment{}", src))
            && let Some(num) = parse_num(raw.trim_start_matches('+'))
            && let Some(json_val) = num_to_json(num)
        {
            color_map.insert("saturation".to_string(), json_val);
        }
        if let Some(raw) = attrs.get(&format!("LuminanceAdjustment{}", src))
            && let Some(num) = parse_num(raw.trim_start_matches('+'))
            && let Some(json_val) = num_to_json(num)
        {
            color_map.insert("luminance".to_string(), json_val);
        }
        if !color_map.is_empty() {
            hsl_map.insert(dst.to_string(), Value::Object(color_map));
        }
    }
    if !hsl_map.is_empty() {
        adjustments.insert("hsl".to_string(), Value::Object(hsl_map));
    }

    let mut shadows_map = Map::new();
    let mut midtones_map = Map::new();
    let mut highlights_map = Map::new();
    let mut global_map = Map::new();
    if let Some(raw) = attrs.get("SplitToningShadowHue")
        && let Some(num) = parse_num(raw)
        && let Some(json_val) = num_to_json(num)
    {
        shadows_map.insert("hue".to_string(), json_val);
    }
    if let Some(raw) = attrs.get("ColorGradeMidtoneHue")
        && let Some(num) = parse_num(raw)
        && let Some(json_val) = num_to_json(num)
    {
        midtones_map.insert("hue".to_string(), json_val);
    }
    if let Some(raw) = attrs.get("SplitToningHighlightHue")
        && let Some(num) = parse_num(raw)
        && let Some(json_val) = num_to_json(num)
    {
        highlights_map.insert("hue".to_string(), json_val);
    }
    if let Some(raw) = attrs.get("SplitToningShadowSaturation")
        && let Some(num) = parse_num(raw)
        && let Some(json_val) = num_to_json(num)
    {
        shadows_map.insert("saturation".to_string(), json_val);
    }
    if let Some(raw) = attrs.get("ColorGradeMidtoneSat")
        && let Some(num) = parse_num(raw)
        && let Some(json_val) = num_to_json(num)
    {
        midtones_map.insert("saturation".to_string(), json_val);
    }
    if let Some(raw) = attrs.get("SplitToningHighlightSaturation")
        && let Some(num) = parse_num(raw)
        && let Some(json_val) = num_to_json(num)
    {
        highlights_map.insert("saturation".to_string(), json_val);
    }
    if let Some(raw) = attrs.get("ColorGradeShadowLum")
        && let Some(num) = parse_num(raw)
        && let Some(json_val) = num_to_json(num)
    {
        shadows_map.insert("luminance".to_string(), json_val);
    }
    if let Some(raw) = attrs.get("ColorGradeMidtoneLum")
        && let Some(num) = parse_num(raw)
        && let Some(json_val) = num_to_json(num)
    {
        midtones_map.insert("luminance".to_string(), json_val);
    }
    if let Some(raw) = attrs.get("ColorGradeHighlightLum")
        && let Some(num) = parse_num(raw)
        && let Some(json_val) = num_to_json(num)
    {
        highlights_map.insert("luminance".to_string(), json_val);
    }
    if let Some(raw) = attrs.get("ColorGradeGlobalHue")
        && let Some(num) = parse_num(raw)
        && let Some(json_val) = num_to_json(num)
    {
        global_map.insert("hue".to_string(), json_val);
    }
    if let Some(raw) = attrs.get("ColorGradeGlobalSat")
        && let Some(num) = parse_num(raw)
        && let Some(json_val) = num_to_json(num)
    {
        global_map.insert("saturation".to_string(), json_val);
    }
    if let Some(raw) = attrs.get("ColorGradeGlobalLum")
        && let Some(num) = parse_num(raw)
        && let Some(json_val) = num_to_json(num)
    {
        global_map.insert("luminance".to_string(), json_val);
    }
    if let Some(raw) = attrs.get("SplitToningBalance")
        && let Some(num) = parse_num(raw)
        && let Some(json_val) = num_to_json(num)
    {
        color_grading_map.insert("balance".to_string(), json_val);
    }
    if !shadows_map.is_empty() {
        color_grading_map.insert("shadows".to_string(), Value::Object(shadows_map));
    }
    if !midtones_map.is_empty() {
        color_grading_map.insert("midtones".to_string(), Value::Object(midtones_map));
    }
    if !highlights_map.is_empty() {
        color_grading_map.insert("highlights".to_string(), Value::Object(highlights_map));
    }
    if !global_map.is_empty() {
        color_grading_map.insert("global".to_string(), Value::Object(global_map));
    }
    if !color_grading_map.is_empty() {
        adjustments.insert("colorGrading".to_string(), Value::Object(color_grading_map));
    }

    let curve_set = crate::mods::lr_legacy::point_curve_prefix(&attrs);
    let curve_mappings = [
        ("", "luma"),
        ("Red", "red"),
        ("Green", "green"),
        ("Blue", "blue"),
    ];
    for (channel, rr_curve) in curve_mappings {
        let xmp_curve = format!("{curve_set}{channel}");
        if let Some(points) = extract_tone_curve_points(xmp_content, &xmp_curve) {
            curves_map.insert(rr_curve.to_string(), Value::Array(points));
        }
    }
    if !curves_map.is_empty() {
        adjustments.insert("curves".to_string(), Value::Object(curves_map));
    }

    let preset_name =
        extract_xmp_name(xmp_content).unwrap_or_else(|| "Imported Preset".to_string());

    Ok(Preset {
        id: Uuid::new_v4().to_string(),
        name: preset_name,
        adjustments: Value::Object(adjustments),
        include_masks: Some(false),
        include_crop_transform: Some(false),
        preset_type: Some("style".to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn imported_shadows(value: &str) -> f64 {
        let xmp = format!(r#"<x:xmpmeta><rdf:Description crs:Shadows2012="{value}"/></x:xmpmeta>"#);
        let preset = convert_xmp_to_preset(&xmp).expect("converts");
        preset.adjustments["shadows"].as_f64().expect("shadows set")
    }

    #[test]
    fn shadows_stay_within_slider_range_at_both_ends() {
        assert_eq!(imported_shadows("-100"), -100.0);
        assert_eq!(imported_shadows("+100"), 100.0);
    }

    #[test]
    fn shadows_inside_range_keep_their_scale() {
        assert_eq!(imported_shadows("-40"), -60.0);
        assert_eq!(imported_shadows("+20"), 30.0);
    }

    /// A process version 2012 preset carrying the old controls and curve
    /// Lightroom writes beside the 2012 ones, which must be ignored.
    const PROCESS_2012_WITH_OLD_KEYS: &str = r#"<x:xmpmeta><rdf:RDF><rdf:Description
        crs:ProcessVersion="11.0" crs:AutoTone="False"
        crs:Exposure="+1.00" crs:Contrast="+40" crs:Brightness="+80" crs:FillLight="30"
        crs:Shadows="9" crs:HighlightRecovery="25" crs:Clarity="+50"
        crs:Exposure2012="+0.35" crs:Contrast2012="-12" crs:Highlights2012="-40"
        crs:Shadows2012="-80" crs:Whites2012="+15" crs:Blacks2012="-7" crs:Clarity2012="+22"
        crs:Temperature="6200" crs:AsShotTemperature="5000" crs:Tint="+8"
        crs:HueAdjustmentRed="-12" crs:SaturationAdjustmentBlue="-30"
        crs:SplitToningShadowHue="210" crs:SplitToningShadowSaturation="15" crs:Sharpness="40">
      <crs:Name><rdf:Alt><rdf:li xml:lang="x-default">Process 2012 with old keys</rdf:li></rdf:Alt></crs:Name>
      <crs:ToneCurve><rdf:Seq><rdf:li>0, 0</rdf:li><rdf:li>128, 160</rdf:li><rdf:li>255, 255</rdf:li></rdf:Seq></crs:ToneCurve>
      <crs:ToneCurvePV2012><rdf:Seq><rdf:li>0, 20</rdf:li><rdf:li>40, 60</rdf:li><rdf:li>255, 240</rdf:li></rdf:Seq></crs:ToneCurvePV2012>
      <crs:ToneCurvePV2012Red><rdf:Seq><rdf:li>0, 0</rdf:li><rdf:li>128, 140</rdf:li><rdf:li>255, 255</rdf:li></rdf:Seq></crs:ToneCurvePV2012Red>
    </rdf:Description></rdf:RDF></x:xmpmeta>"#;

    fn xmp_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).expect("preset folder") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                xmp_files(&path, out);
            } else if path.extension().is_some_and(|e| e == "xmp") {
                out.push(path);
            }
        }
    }

    /// Presets from process version 2012 on convert exactly as they did
    /// before older process versions were read: the 26 bundled presets and
    /// one carrying the old controls too. The hash is of this converter's
    /// output before that change.
    #[test]
    fn process_2012_presets_convert_as_before() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/presets");
        let mut files = Vec::new();
        xmp_files(&root, &mut files);
        files.sort();
        assert_eq!(files.len(), 26);
        let mut inputs: Vec<(String, String)> = files
            .iter()
            .map(|path| {
                let name = path
                    .strip_prefix(&root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                (name, std::fs::read_to_string(path).expect("preset"))
            })
            .collect();
        inputs.push(("inline".into(), PROCESS_2012_WITH_OLD_KEYS.into()));
        let mut text = String::new();
        for (name, xmp) in &inputs {
            let preset = convert_xmp_to_preset(xmp).expect("converts");
            let json = serde_json::to_string(&(&preset.name, &preset.adjustments)).unwrap();
            text.push_str(&format!("{name}\n{json}\n"));
        }
        let hash = hex::encode(<sha2::Sha256 as sha2::Digest>::digest(text.as_bytes()));
        assert_eq!(
            hash, "5b8117d6f133e9524eec9e0fc7461ef673a08ec70d268dbbe056d0cd1edbc51c",
            "{text}"
        );
    }

    /// A Lightroom 3 preset (process version 2010) as Lightroom 5 saves it:
    /// the 2012 keys stored at 0 and a linear 2012 curve beside its own.
    const PROCESS_2010_PRESET: &str = r#"<x:xmpmeta><rdf:RDF><rdf:Description
        crs:ProcessVersion="5.7" crs:Exposure="+0.50" crs:Contrast="+40"
        crs:Brightness="+60" crs:FillLight="20" crs:Shadows="2" crs:HighlightRecovery="15"
        crs:Clarity="+30" crs:Exposure2012="0.00" crs:Contrast2012="0" crs:Shadows2012="0"
        crs:Clarity2012="0" crs:Vibrance="+10" crs:ToneCurveName="Medium Contrast">
      <crs:Name><rdf:Alt><rdf:li xml:lang="x-default">LR3 look</rdf:li></rdf:Alt></crs:Name>
      <crs:ToneCurve><rdf:Seq><rdf:li>0, 0</rdf:li><rdf:li>32, 22</rdf:li><rdf:li>64, 56</rdf:li><rdf:li>128, 128</rdf:li><rdf:li>192, 196</rdf:li><rdf:li>255, 255</rdf:li></rdf:Seq></crs:ToneCurve>
      <crs:ToneCurveRed><rdf:Seq><rdf:li>0, 0</rdf:li><rdf:li>128, 150</rdf:li><rdf:li>255, 255</rdf:li></rdf:Seq></crs:ToneCurveRed>
      <crs:ToneCurvePV2012><rdf:Seq><rdf:li>0, 0</rdf:li><rdf:li>255, 255</rdf:li></rdf:Seq></crs:ToneCurvePV2012>
    </rdf:Description></rdf:RDF></x:xmpmeta>"#;

    /// Exposure, Contrast, Fill Light, Recovery, Shadows, Clarity and the tone
    /// curve of an old preset survive the import, converted as Lightroom 5's
    /// process update writes them (RAWmakase eb8a3356, c83741ea).
    #[test]
    fn process_2010_presets_keep_their_tone_controls_and_curve() {
        let preset = convert_xmp_to_preset(PROCESS_2010_PRESET).expect("converts");
        let a = &preset.adjustments;
        let value = |key: &str| a[key].as_f64().unwrap_or_else(|| panic!("{key} in {a}"));
        // Exposure 0.5, Brightness +10 (0.2 EV) and Fill Light 20 (0.4 EV).
        assert_eq!(value("exposure"), 1.1);
        assert_eq!(value("contrast"), 20.0);
        // Fill Light 20 lifts shadows by 10 (scaled 1.5 on import) and holds highlights.
        assert_eq!(value("shadows"), 15.0);
        assert_eq!(value("highlights"), -10.0);
        assert_eq!(value("whites"), -15.0);
        assert_eq!(value("blacks"), 15.0);
        assert_eq!(value("clarity"), 15.0);
        assert_eq!(value("vibrance"), 10.0);
        let points = |channel: &str| -> Vec<(u64, u64)> {
            a["curves"][channel]
                .as_array()
                .unwrap_or_else(|| panic!("{channel} curve in {a}"))
                .iter()
                .map(|p| (p["x"].as_u64().unwrap(), p["y"].as_u64().unwrap()))
                .collect()
        };
        let medium_contrast = [
            (0, 0),
            (32, 22),
            (64, 56),
            (128, 128),
            (192, 196),
            (255, 255),
        ];
        assert_eq!(points("luma"), medium_contrast);
        assert_eq!(points("red"), [(0, 0), (128, 150), (255, 255)]);
    }
}
