//! Writes this version's folder of the compatibility corpus (compat_corpus.rs):
//! `src-tauri/fixtures/compat/<version>/`. Ours, test-only.
//!
//!     cargo test --lib mods::compat_fixture -- --ignored
//!
//! The settings and the edit are written through `AppSettings` and
//! `ImageMetadata`, as save_settings and write_sidecar write them, so the
//! files are in this version's shape. Every setting and every key of the
//! editor's starting values holds a valid value other than its default
//! (`compat_corpus::uncovered` says which may not, and why); the edit has a
//! mask of each kind the Masks panel makes, an AI patch of each kind, three
//! Snapshots and a History. A value that would switch others off in the
//! render (B&W, a hidden section, lens corrections off) is switched back on
//! in a Snapshot, so the corpus reads those others too.
//!
//! Deterministic: fixed ids and times, images drawn here, sets sorted. Nothing
//! is read from a photo. Every coordinate is in a photo of
//! `compat_snapshot::PHOTO_PX`.

use std::io::Cursor;

use base64::{Engine as _, engine::general_purpose};
use image::{DynamicImage, GrayImage, ImageFormat, Luma, Rgb, RgbImage, Rgba, RgbaImage};
use serde_json::{Value, json};

use crate::app_settings::{AppSettings, all_available_adjustments, default_included_adjustments};
use crate::image_processing::{ImageFlag, ImageMetadata};
use crate::mods::compat_corpus::{EDIT, SETTINGS, UNORDERED, root, uncovered};
use crate::mods::compat_snapshot::PHOTO_PX;
use crate::mods::history::HistoryRecord;
use crate::mods::snapshots::Snapshot;

#[test]
#[ignore = "writes src-tauri/fixtures/compat/<version>: run it on purpose"]
fn write_this_release() {
    let release = env!("CARGO_PKG_VERSION");
    let settings = settings();
    let metadata = metadata();
    let missing = uncovered(&settings, &metadata["adjustments"]);
    assert!(
        missing.is_empty(),
        "give each a value other than its default here:\n  {}",
        missing.join("\n  ")
    );
    let dir = root().join(release);
    let readme = std::fs::read_to_string(dir.join("README.md")).unwrap_or_default();
    assert!(
        !readme.contains("has shipped"),
        "{release} has shipped and its folder is never written again: bump the version first"
    );
    std::fs::create_dir_all(&dir).expect("the release folder");
    let pretty = |value: &Value| serde_json::to_string_pretty(value).expect("writes") + "\n";
    std::fs::write(dir.join(SETTINGS), pretty(&settings)).expect("settings.json");
    std::fs::write(dir.join(EDIT), pretty(&metadata)).expect("photo.agdata");
    std::fs::write(
        dir.join("README.md"),
        format!(
            "# {release}\n\n\
             Written by `write_this_release` in `src-tauri/src/mods/compat_fixture.rs` from the\n\
             code of {release}: every setting and every key of the editor's starting values set to\n\
             a valid value other than its default, a mask of each kind the Masks panel makes, an\n\
             AI patch of each kind, three Snapshots and a History. Synthetic: no photo was read.\n\n\
             While {release} is unreleased the generator may write this folder again; once it has\n\
             shipped, this folder is never edited.\n"
        ),
    )
    .expect("README.md");
    println!("wrote {}", dir.display());
}

/// Through `AppSettings`, as save_settings writes it; its sets sorted.
fn settings() -> Value {
    let mut literal: Value = serde_json::from_str(SETTINGS_JSON).expect("SETTINGS_JSON");
    let mut included: Vec<String> = default_included_adjustments()
        .into_iter()
        .filter(|key| !["toneMapper", "lutPath", "lutName", "lutData"].contains(&key.as_str()))
        .chain(["crop".to_string(), "masks".to_string()])
        .collect();
    included.sort();
    // As a version before the film-look key wrote it.
    let mut known: Vec<String> = all_available_adjustments()
        .into_iter()
        .filter(|key| key != "agLutDeclaredInput")
        .collect();
    known.sort();
    literal["copyPasteSettings"] = json!({
        "mode": "replace",
        "includedAdjustments": included,
        "knownAdjustments": known,
        "autoSync": true,
    });
    let settings: AppSettings = serde_json::from_value(literal).expect("reads as AppSettings");
    let mut value: Value =
        serde_json::from_str(&serde_json::to_string(&settings).expect("writes")).expect("reads");
    for (file, path) in UNORDERED {
        if *file == SETTINGS
            && let Some(Value::Array(items)) = value.pointer_mut(path)
        {
            items.sort_by_key(Value::to_string);
        }
    }
    value
}

/// Through `ImageMetadata`, as write_sidecar writes it.
fn metadata() -> Value {
    let edit = edit();
    // The photo's edit with a few values changed, its masks and patches left
    // out: the photo's own edit already holds them.
    let variant = |changes: Value| {
        let mut changed = edit.clone();
        merge(&mut changed, &changes);
        changed["masks"] = json!([]);
        changed["aiPatches"] = json!([]);
        changed
    };
    let snapshots = vec![
        snapshot(
            1,
            "Colour, sharpening and lens on, no masks",
            variant(json!({
                "agBlackWhite": { "enabled": false },
                "agSharpen": { "enabled": true, "capture": true },
                "sectionVisibility": { "effects": true },
                "lensBuiltin": false,
                "lensDistortionEnabled": true,
                "lensTcaEnabled": true,
                "lensVignetteEnabled": true,
                "showClipping": 0,
            })),
        ),
        snapshot(
            2,
            "Every section hidden, no masks",
            variant(json!({
                "sectionVisibility": {
                    "basic": false, "curves": false, "color": false, "details": false, "effects": false
                },
            })),
        ),
        // What Reset writes (reset_adjustments_for_paths).
        snapshot(3, "Reset", json!({ "baselineExposure": true })),
    ];
    let history: HistoryRecord = serde_json::from_value(json!({
        "end": 2,
        "steps": [
            { "name": "Initial State" },
            { "name": "Exposure", "set": { "/exposure": 0.65 } },
            { "name": "Mask Opacity", "set": { "/masks/0/opacity": 85 } },
            { "name": "Crop", "unset": ["/crop"] },
        ],
    }))
    .expect("a History");
    let history = HistoryRecord {
        edit: crate::mods::history::fingerprint(&edit),
        ..history
    };
    let exif: std::collections::HashMap<String, String> = [
        ("Make", "Corpus"),
        ("Model", "Corpus One"),
        ("LensModel", "Corpus 35mm f/2"),
        ("FNumber", "f/2.8"),
        ("ExposureTime", "1/250 s"),
        ("PhotographicSensitivity", "400"),
        ("FocalLength", "35 mm"),
        ("DateTimeOriginal", "2026:10:14 10:20:30"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    let metadata = ImageMetadata {
        version: 1,
        rating: 4,
        flag: Some(ImageFlag::Pick),
        adjustments: edit,
        tags: Some(vec!["corpus".to_string(), "fixture".to_string()]),
        exif: Some(exif),
        ag_snapshots: Some(snapshots),
        ag_history: Some(history),
    };
    serde_json::from_str(&serde_json::to_string(&metadata).expect("writes")).expect("reads")
}

fn snapshot(n: u64, name: &str, adjustments: Value) -> Snapshot {
    Snapshot {
        id: format!("snapshot-{n}"),
        name: name.to_string(),
        at: 1_792_000_000_000 + n * 60_000,
        adjustments,
    }
}

/// The photo's edit: every starting value changed, masks and AI patches added.
fn edit() -> Value {
    let mut edit: Value = serde_json::from_str(EDIT_JSON).expect("EDIT_JSON");
    edit["masks"] = Value::Array(masks());
    edit["aiPatches"] = Value::Array(patches());
    fill_images(&mut edit);
    edit
}

/// Settings over `into`, objects key by key.
fn merge(into: &mut Value, changes: &Value) {
    match (into, changes) {
        (Value::Object(into), Value::Object(changes)) => {
            for (key, change) in changes {
                match into.get_mut(key) {
                    Some(slot @ Value::Object(_)) if change.is_object() => merge(slot, change),
                    _ => {
                        into.insert(key.clone(), change.clone());
                    }
                }
            }
        }
        (into, changes) => *into = changes.clone(),
    }
}

/// The orientation the edit has, as the interface copies it into the
/// parameters of a mask drawn from a whole picture.
fn oriented(mut parameters: Value) -> Value {
    merge(
        &mut parameters,
        &json!({
            "rotation": 2.5, "flipHorizontal": true, "flipVertical": true, "orientationSteps": 1
        }),
    );
    parameters
}

fn sub_mask(
    id: &str,
    kind: &str,
    name: &str,
    mode: &str,
    invert: bool,
    parameters: Value,
) -> Value {
    json!({
        "id": id,
        "name": name,
        "type": kind,
        "visible": true,
        "invert": invert,
        "opacity": 90,
        "mode": mode,
        "parameters": parameters,
    })
}

fn brush_lines() -> Value {
    json!([
        {
            "tool": "brush", "brushSize": 40, "feather": 0.5, "flow": 100,
            "points": [{ "x": 100, "y": 150 }, { "x": 160, "y": 200 }, { "x": 220, "y": 260 }]
        },
        {
            "tool": "eraser", "brushSize": 20, "feather": 0.3, "flow": 100,
            "points": [{ "x": 150, "y": 190 }, { "x": 170, "y": 210 }]
        }
    ])
}

fn radial() -> Value {
    json!({
        "centerX": 200, "centerY": 260, "radiusX": 120, "radiusY": 160,
        "rotation": 15, "feather": 0.6
    })
}

fn linear() -> Value {
    json!({ "startX": 60, "startY": 100, "endX": 340, "endY": 420, "range": 80 })
}

/// One mask of each kind the Masks panel makes, and one of three kinds at
/// once in each mode.
fn masks() -> Vec<Value> {
    let ai = |selection: &str, image: &str, grow: i32, feather: i32| {
        oriented(json!({
            "agSelection": selection, "maskDataBase64": image, "grow": grow, "feather": feather
        }))
    };
    let kinds: Vec<(&str, Vec<Value>)> = vec![
        (
            "Radial",
            vec![sub_mask(
                "sub-0",
                "radial",
                "Radial",
                "additive",
                false,
                radial(),
            )],
        ),
        (
            "Linear",
            vec![sub_mask(
                "sub-1",
                "linear",
                "Linear",
                "additive",
                false,
                linear(),
            )],
        ),
        (
            "Brush",
            vec![sub_mask(
                "sub-2",
                "brush",
                "Brush",
                "additive",
                false,
                json!({ "lines": brush_lines() }),
            )],
        ),
        (
            "Flow",
            vec![sub_mask(
                "sub-3",
                "flow",
                "Flow",
                "additive",
                false,
                json!({
                    "flow": 30,
                    "lines": [{
                        "tool": "brush", "brushSize": 50, "feather": 0.7, "flow": 30,
                        "points": [{ "x": 80, "y": 400 }, { "x": 200, "y": 380 }, { "x": 320, "y": 420 }]
                    }]
                }),
            )],
        ),
        (
            "Colour",
            vec![sub_mask(
                "sub-4",
                "color",
                "Color",
                "additive",
                false,
                oriented(json!({
                    "targetX": 150, "targetY": 300, "tolerance": 35, "grow": 10, "feather": 40
                })),
            )],
        ),
        (
            "Luminance",
            vec![sub_mask(
                "sub-5",
                "luminance",
                "Luminance",
                "additive",
                false,
                oriented(json!({
                    "targetX": 250, "targetY": 120, "tolerance": 25, "grow": -10, "feather": 30
                })),
            )],
        ),
        (
            "Whole image",
            vec![sub_mask(
                "sub-6",
                "all",
                "Whole Image",
                "additive",
                false,
                json!({}),
            )],
        ),
        (
            "Depth",
            vec![sub_mask(
                "sub-7",
                "ai-depth",
                "Depth",
                "additive",
                false,
                oriented(json!({
                    "minDepth": 25, "maxDepth": 75, "minFade": 10, "maxFade": 20, "feather": 15,
                    "maskDataBase64": "@mask:depth"
                })),
            )],
        ),
        (
            "Subject",
            vec![sub_mask(
                "sub-8",
                "ai-foreground",
                "Subject",
                "additive",
                false,
                ai("subject", "@mask:subject", 5, 10),
            )],
        ),
        (
            "Sky",
            vec![sub_mask(
                "sub-9",
                "ai-sky",
                "Sky",
                "additive",
                false,
                ai("sky", "@mask:sky", -5, 20),
            )],
        ),
        (
            "Background",
            vec![sub_mask(
                "sub-10",
                "ai-foreground",
                "Background",
                "additive",
                false,
                ai("background", "@mask:background", 0, 25),
            )],
        ),
        (
            "Object, less a circle, within a band",
            vec![
                sub_mask("sub-11a", "ai-foreground", "Object", "additive", false, {
                    let mut p = ai("object", "@mask:object", 8, 12);
                    p["agClicks"] = json!([
                        { "x": 120, "y": 200, "positive": true },
                        { "x": 300, "y": 450, "positive": false }
                    ]);
                    p["agBounds"] =
                        json!({ "startX": 80, "startY": 150, "endX": 320, "endY": 480 });
                    p
                }),
                sub_mask("sub-11b", "radial", "Radial", "subtractive", true, radial()),
                sub_mask("sub-11c", "linear", "Linear", "intersect", false, linear()),
            ],
        ),
        (
            "Text",
            vec![sub_mask(
                "sub-12",
                "ai-foreground",
                "Text",
                "additive",
                false,
                {
                    let mut p = ai("text", "@mask:text", 2, 4);
                    p["agWords"] = json!("OPEN");
                    p
                },
            )],
        ),
        (
            "Face",
            vec![sub_mask(
                "sub-13",
                "ai-foreground",
                "Face",
                "additive",
                false,
                {
                    let mut p = ai("face", "@mask:face", 0, 6);
                    p["agParts"] = json!(["faceSkin", "lips", "hair"]);
                    p
                },
            )],
        ),
    ];
    kinds
        .into_iter()
        .enumerate()
        .map(|(i, (name, sub_masks))| {
            json!({
                "id": format!("mask-{i}"),
                "name": name,
                "visible": true,
                "invert": i % 2 == 1,
                "opacity": if name == "Whole image" { 40 } else { 85 },
                "adjustments": mask_adjustments(i),
                "subMasks": sub_masks,
            })
        })
        .collect()
}

/// Every key of the interface's INITIAL_MASK_ADJUSTMENTS changed, and the
/// mask's own section eyes and Sharpen; the exposure tells the masks apart.
fn mask_adjustments(i: usize) -> Value {
    let mut adjustments: Value =
        serde_json::from_str(MASK_ADJUSTMENTS_JSON).expect("MASK_ADJUSTMENTS_JSON");
    adjustments["exposure"] = json!((2 + i) as f64 / 10.0);
    adjustments
}

/// One AI patch of each kind.
fn patches() -> Vec<Value> {
    let patch_data = json!({
        "color": "@patch:color", "mask": "@patch:mask",
        "offsetX": 100, "offsetY": 120, "width": 60, "height": 40, "isSrgbEncoded": true
    });
    let boxed = |image: &str, grow: i32, feather: i32| {
        oriented(json!({
            "startX": 100, "startY": 120, "endX": 160, "endY": 160,
            "maskDataBase64": image, "grow": grow, "feather": feather
        }))
    };
    let patch = |n: usize, name: &str, prompt: &str, data: &Value, sub_masks: Vec<Value>| {
        json!({
            "id": format!("patch-{n}"),
            "name": name,
            "visible": true,
            "invert": false,
            "isLoading": false,
            "prompt": prompt,
            "patchData": data,
            "subMasks": sub_masks,
        })
    };
    vec![
        patch(
            1,
            "Quick erase",
            "",
            &patch_data,
            vec![sub_mask(
                "patch-sub-1",
                "quick-eraser",
                "Quick Eraser",
                "additive",
                false,
                boxed("@mask:object", 75, 75),
            )],
        ),
        patch(
            2,
            "Replace",
            "a stone wall",
            &patch_data,
            vec![
                sub_mask(
                    "patch-sub-2a",
                    "ai-subject",
                    "Subject",
                    "additive",
                    false,
                    boxed("@mask:subject", 50, 25),
                ),
                sub_mask(
                    "patch-sub-2b",
                    "brush",
                    "Brush",
                    "subtractive",
                    false,
                    json!({ "lines": brush_lines() }),
                ),
            ],
        ),
        patch(
            3,
            "Clone",
            "",
            &Value::Null,
            vec![sub_mask(
                "patch-sub-3",
                "clone",
                "Clone",
                "additive",
                false,
                json!({ "lines": brush_lines(), "sourceX": 300, "sourceY": 220 }),
            )],
        ),
        patch(
            4,
            "Heal",
            "",
            &Value::Null,
            vec![sub_mask(
                "patch-sub-4",
                "heal",
                "Heal",
                "additive",
                false,
                json!({ "lines": brush_lines(), "sourceX": 260, "sourceY": 180 }),
            )],
        ),
        patch(
            5,
            "Liquify",
            "",
            &Value::Null,
            vec![sub_mask(
                "patch-sub-5",
                "liquify",
                "Liquify",
                "additive",
                false,
                json!({ "pressure": 60, "liquifyMode": "twirl", "lines": brush_lines() }),
            )],
        ),
        patch(
            6,
            "Retouch",
            "",
            &Value::Null,
            vec![sub_mask(
                "patch-sub-6",
                "retouch",
                "Retouch",
                "additive",
                false,
                json!({ "intensity": 55, "lines": brush_lines() }),
            )],
        ),
    ]
}

/// Each `"@name"` string replaced by the image it names.
fn fill_images(value: &mut Value) {
    match value {
        Value::String(text) if text.starts_with('@') => *text = image(&text[1..]),
        Value::Array(items) => items.iter_mut().for_each(fill_images),
        Value::Object(map) => map.values_mut().for_each(fill_images),
        _ => {}
    }
}

/// The images an edit holds, drawn here, as the app stores each.
fn image(name: &str) -> String {
    let (w, h) = PHOTO_PX;
    // Hard-edged, so the PNG stays small; grow and feather soften it.
    let ellipse = |cx: f32, cy: f32, rx: f32, ry: f32| {
        move |x: u32, y: u32| {
            let d = ((x as f32 - cx) / rx).powi(2) + ((y as f32 - cy) / ry).powi(2);
            Luma([if d <= 1.0 { 255 } else { 0 }])
        }
    };
    let gray = |img: GrayImage| png(DynamicImage::ImageLuma8(img));
    // A mask the size of the photo, as the AI masks are stored.
    let mask = |img: GrayImage| format!("data:image/png;base64,{}", gray(img));
    // A small map of the picture, as depth and normal maps are stored.
    let map = |img: DynamicImage| format!("data:image/png;space=source;base64,{}", png(img));
    match name {
        "mask:subject" => mask(GrayImage::from_fn(
            w,
            h,
            ellipse(300.0, 200.0, 150.0, 120.0),
        )),
        "mask:object" => mask(GrayImage::from_fn(w, h, ellipse(180.0, 260.0, 70.0, 50.0))),
        "mask:text" => mask(GrayImage::from_fn(w, h, |x, y| {
            Luma([if (420..540).contains(&x) && (60..100).contains(&y) {
                255
            } else {
                0
            }])
        })),
        "mask:face" => mask(GrayImage::from_fn(w, h, ellipse(330.0, 150.0, 45.0, 60.0))),
        "mask:background" => {
            let subject = ellipse(300.0, 200.0, 150.0, 120.0);
            mask(GrayImage::from_fn(w, h, |x, y| {
                Luma([255 - subject(x, y)[0]])
            }))
        }
        "mask:sky" => mask(GrayImage::from_fn(w, h, |_, y| {
            Luma([(255 - (y * 255 * 5 / (h * 2)).min(255)) as u8])
        })),
        "mask:depth" => mask(GrayImage::from_fn(w, h, |_, y| {
            Luma([(y * 255 / (h - 1)) as u8])
        })),
        "map:depth" => map(DynamicImage::ImageLuma8(GrayImage::from_fn(
            60,
            40,
            |x, y| Luma([(x * 4 + y * 2) as u8]),
        ))),
        "map:normal" => map(DynamicImage::ImageRgba8(RgbaImage::from_fn(
            60,
            40,
            |x, y| Rgba([(x * 4) as u8, (y * 6) as u8, 200, 255]),
        ))),
        // An AI patch's pixels and mask, as inpainting.rs returns them: no prefix.
        "patch:color" => png(DynamicImage::ImageRgb8(RgbImage::from_fn(
            60,
            40,
            |x, y| Rgb([(x * 4) as u8, (y * 6) as u8, 90]),
        ))),
        "patch:mask" => gray(GrayImage::from_fn(60, 40, ellipse(30.0, 20.0, 25.0, 15.0))),
        _ => panic!("no image is called {name}"),
    }
}

fn png(image: DynamicImage) -> String {
    let mut bytes = Cursor::new(Vec::new());
    image
        .write_to(&mut bytes, ImageFormat::Png)
        .expect("encodes");
    general_purpose::STANDARD.encode(bytes.into_inner())
}

/// Every setting, none at its default. `copyPasteSettings` is filled in by
/// `settings()` from the app's own lists.
const SETTINGS_JSON: &str = r##"{
  "lastRootPath": "/photos/2026",
  "rootFolders": ["/photos/2026", "/photos/archive"],
  "pinnedFolders": ["/photos/2026/october"],
  "editorPreviewResolution": 2560,
  "smallThumbnailResolution": 360,
  "mediumThumbnailResolution": 1600,
  "enableZoomHifi": false,
  "useFullDpiRendering": true,
  "highResZoomMultiplier": 1.5,
  "enableLivePreviews": false,
  "livePreviewQuality": "medium",
  "sortCriteria": { "key": "rating", "order": "desc" },
  "filterCriteria": {
    "rating": 3, "rawStatus": "rawOnly", "editedStatus": "editedOnly", "flagStatus": "picked",
    "colors": ["red", "blue"], "ratingOperator": "eq"
  },
  "theme": "grey",
  "fontFamily": "system",
  "decorations": true,
  "aiConnectorAddress": "http://127.0.0.1:8188",
  "lastFolderState": {
    "currentFolderPath": "/photos/2026/october", "expandedFolders": ["/photos/2026"],
    "activeAlbumId": "album-1", "expandedAlbumGroups": ["group-1"]
  },
  "uiVisibility": { "filmstrip": false, "leftPanel": true, "rightPanel": false },
  "enableAiTagging": true,
  "taggingThreadCount": 2,
  "taggingShortcuts": ["portrait", "street", "corpus"],
  "customAiTags": ["corpus", "fixture"],
  "aiTagCount": 6,
  "thumbnailSize": "large",
  "thumbnailAspectRatio": "cover",
  "aiProvider": "cloud",
  "openTreeSections": ["current", "pinned"],
  "copyPasteSettings": null,
  "rawHighlightCompression": 1.5,
  "processingBackend": "gl",
  "linuxGpuOptimization": true,
  "linuxGpuOptimizationMigratedV1": false,
  "libraryViewMode": "recursive",
  "exportPresets": [{
    "id": "corpus-tiff", "name": "Corpus TIFF", "fileFormat": "tiff", "jpegQuality": 88,
    "enableResize": true, "resizeMode": "longEdge", "resizeValue": 3000, "dontEnlarge": false,
    "keepMetadata": true, "stripGps": true, "filenameTemplate": "{original_filename}_corpus",
    "enableWatermark": true, "watermarkPath": "/photos/watermark.png", "watermarkAnchor": "topLeft",
    "watermarkScale": 12, "watermarkSpacing": 4, "watermarkOpacity": 60, "exportMasks": true,
    "preserveFolders": true, "enablePad": true, "padRatioWidth": 4.0, "padRatioHeight": 5.0,
    "padColor": "#101010", "enableBorder": true, "borderBasis": "shortEdge",
    "borderHorizontalPercent": 3.0, "borderVerticalPercent": 4.5, "borderColor": "#f0f0f0",
    "lastExportPath": "/exports", "destinationType": "originalFolder", "subfolder": "corpus",
    "tiffBitDepth": 8, "preserveTimestamps": true
  }],
  "myLenses": [{ "maker": "Corpus", "model": "Corpus 35mm f/2", "correctNew": true }],
  "enableFolderImageCounts": true,
  "displayEditIcon": false,
  "linearRawMode": "gamma",
  "enableXmpSync": false,
  "createXmpIfMissing": true,
  "isWaveformVisible": true,
  "waveformHeight": 300,
  "activeWaveformChannel": "rgb",
  "useWgpuRenderer": false,
  "editorNeutralGreyBg": true,
  "canvasInputMode": "trackpad",
  "whiteBalanceMode": "kelvin",
  "zoomSpeedMultiplier": 1.25,
  "zoomPhotoToPixelClick": true,
  "keybinds": { "open_image": ["ctrl", "Enter"], "copy_files": ["ctrl", "alt", "KeyC"] },
  "thumbnailWorkerThreads": 6,
  "imageCacheSize": 8,
  "tonemapperOverrideEnabled": true,
  "defaultRawTonemapper": "basic",
  "defaultNonRawTonemapper": "agx",
  "enableFocusMode": true,
  "enableToolFocusMode": true,
  "folderIcons": { "/photos/2026": "camera" },
  "rawPreprocessingColorNr": 0.25,
  "rawPreprocessingSharpening": 0.5,
  "applyPreprocessingToNonRaws": true,
  "useAppleRaw9": true,
  "exifOverlay": "hover",
  "language": "fr",
  "folderTreeSort": { "key": "modified", "order": "desc" },
  "libraryDisplayMode": "list",
  "grouping": "raw",
  "alwaysDecodeRawThumbnails": true,
  "customAspectRatios": [{ "width": 5.0, "height": 7.0 }],
  "adjustmentLayout": {
    "sectionOrder": ["color", "basic", "curves", "details", "effects"],
    "hiddenSections": ["effects"],
    "openSections": { "basic": true, "color": false },
    "toolOrder": { "color": ["hue", "whiteBalance"] },
    "hiddenTools": ["colorCalibration"],
    "collapsedTools": ["grain"]
  },
  "workspace": {
    "leftPanelWidth": 280, "rightPanelWidth": 360
  }
}"##;

/// Every starting value of the edit, none at its default, and the keys the
/// interface and the app add as they are used. Images are `"@name"`.
const EDIT_JSON: &str = r##"{
  "activeTools": { "chromaticAberration": false, "colorCalibration": true },
  "aiDenoiseAmount": 60, "aiDenoiseDetail": 30, "aiDenoiseEnabled": true,
  "aspectRatio": 0.6666666666666666,
  "baselineExposure": false,
  "agDngColour": false,
  "blacks": -12, "brightness": 0.35,
  "centré": 18, "centréX": 0.42, "centréY": 0.58,
  "clarity": 22,
  "chromaticAberrationBlueYellow": -7, "chromaticAberrationRedCyan": 9,
  "colorCalibration": {
    "shadowsTint": 6, "redHue": -11, "redSaturation": 14, "greenHue": 8, "greenSaturation": -9,
    "blueHue": 12, "blueSaturation": 7
  },
  "colorGrading": {
    "balance": -15, "blending": 65,
    "global": { "hue": 40, "saturation": 12, "luminance": -5 },
    "highlights": { "hue": 55, "saturation": 18, "luminance": 6 },
    "midtones": { "hue": 200, "saturation": 9, "luminance": -3 },
    "shadows": { "hue": 220, "saturation": 21, "luminance": 4 },
    "agLightroom": 1
  },
  "colorNoiseReduction": 25, "contrast": 18,
  "crop": { "unit": "px", "x": 20, "y": 30, "width": 340, "height": 510 },
  "curves": {
    "blue": [{ "x": 0, "y": 10 }, { "x": 255, "y": 245 }],
    "green": [{ "x": 0, "y": 0 }, { "x": 128, "y": 124 }, { "x": 255, "y": 250 }],
    "luma": [{ "x": 0, "y": 0 }, { "x": 64, "y": 58 }, { "x": 192, "y": 200 }, { "x": 255, "y": 255 }],
    "red": [{ "x": 0, "y": 4 }, { "x": 128, "y": 132 }, { "x": 255, "y": 255 }]
  },
  "pointCurves": {
    "blue": [{ "x": 0, "y": 6 }, { "x": 255, "y": 250 }],
    "green": [{ "x": 0, "y": 0 }, { "x": 96, "y": 90 }, { "x": 255, "y": 255 }],
    "luma": [{ "x": 0, "y": 0 }, { "x": 128, "y": 140 }, { "x": 255, "y": 255 }],
    "red": [{ "x": 0, "y": 0 }, { "x": 160, "y": 150 }, { "x": 255, "y": 255 }]
  },
  "parametricCurve": {
    "luma": { "darks": 10, "shadows": -8, "highlights": 6, "lights": -4, "whiteLevel": -5, "blackLevel": 3, "split1": 20, "split2": 45, "split3": 70 },
    "red": { "darks": 4, "shadows": 2, "highlights": -3, "lights": 1, "whiteLevel": -2, "blackLevel": 1, "split1": 30, "split2": 50, "split3": 80 },
    "green": { "darks": -2, "shadows": 3, "highlights": 2, "lights": -1, "whiteLevel": -1, "blackLevel": 2, "split1": 25, "split2": 55, "split3": 75 },
    "blue": { "darks": 6, "shadows": -4, "highlights": 4, "lights": -2, "whiteLevel": -3, "blackLevel": 4, "split1": 15, "split2": 40, "split3": 65 }
  },
  "curveMode": "parametric",
  "dehaze": 14, "exposure": 0.65,
  "flipHorizontal": true, "flipVertical": true,
  "flareAmount": 12, "glowAmount": 20, "grainAmount": 15, "grainRoughness": 60, "grainSize": 35,
  "guidedPerspective": {
    "enabled": true,
    "lines": [
      { "id": "guide-1", "type": "vertical", "p1": { "x": 0.2, "y": 0.1 }, "p2": { "x": 0.22, "y": 0.9 } },
      { "id": "guide-2", "type": "vertical", "p1": { "x": 0.8, "y": 0.1 }, "p2": { "x": 0.77, "y": 0.9 } }
    ],
    "autoCrop": false
  },
  "halationAmount": 18, "highlights": -35,
  "hsl": {
    "aquas": { "hue": 5, "saturation": -10, "luminance": 4 },
    "blues": { "hue": -8, "saturation": 12, "luminance": -6 },
    "greens": { "hue": 10, "saturation": -15, "luminance": 3 },
    "magentas": { "hue": -4, "saturation": 6, "luminance": -2 },
    "oranges": { "hue": 3, "saturation": 9, "luminance": 7 },
    "purples": { "hue": 6, "saturation": -5, "luminance": 1 },
    "reds": { "hue": -6, "saturation": 14, "luminance": -3 },
    "yellows": { "hue": 2, "saturation": -7, "luminance": 5 }
  },
  "hue": 7,
  "lensBlurAmount": 55, "lensBlurDiffusion": 20, "lensBlurShape": "hexagon",
  "lensBlurDepthMap": "@map:depth", "lensBlurEnabled": true,
  "lensBlurMaxDepth": 85, "lensBlurMaxFade": 10, "lensBlurMinDepth": 30, "lensBlurMinFade": 25,
  "relightEnabled": true, "relightNormalMap": "@map:normal",
  "relightLights": [{
    "id": "light-1", "type": "spot", "x": 0.3, "y": 0.4, "depth": 20, "intensity": 70,
    "radius": 35, "angle": 120, "elevation": 45, "cone": 50, "feather": 40,
    "temperature": 15, "tint": -5, "color": "#ffd8a8"
  }],
  "relightAmbient": -10, "relightSoftness": 35, "relightShine": 15, "relightShadows": true,
  "relightShadowSoftness": 25,
  "fogEnabled": true, "fogDepthMap": "@map:depth", "fogAmount": 40, "fogStart": 10,
  "fogDensity": 60, "fogHeight": 20, "fogVariation": 30, "fogGlow": 15, "fogTemperature": -12,
  "fogTint": 6,
  "lensCorrectionMode": "auto",
  "lensDistortionAmount": 80, "lensVignetteAmount": 120, "lensTcaAmount": 90,
  "lensDistortionEnabled": false, "lensTcaEnabled": false, "lensVignetteEnabled": false,
  "lensDistortionParams": {
    "k1": -0.012, "k2": 0.004, "k3": -0.001, "model": 0, "tca_vr": 1.0003, "tca_vb": 0.9998,
    "vig_k1": -0.3, "vig_k2": 0.1, "vig_k3": -0.02
  },
  "lensMaker": "Corpus", "lensModel": "Corpus 35mm f/2",
  "lensBuiltin": true,
  "lensBuiltinParams": {
    "source": "Corpus built-in", "default_on": true,
    "vignetting": { "knots": [0, 0.5, 1], "values": [1, 1.15, 1.6] },
    "distortion": { "knots": [0, 0.5, 1], "values": [1, 0.995, 0.985] },
    "chromatic": [
      { "knots": [0, 1], "values": [1, 1.0004] },
      { "knots": [0, 1], "values": [1, 0.9997] }
    ]
  },
  "lumaNoiseReduction": 20,
  "lutData": null, "lutIntensity": 140, "lutName": "Corpus Film", "lutPath": "/luts/corpus-film.cube",
  "lutSize": 33, "lutIsSceneReferred": true,
  "agLutDeclaredInput": true, "agLutExposureEv": 0.5,
  "masks": [], "aiPatches": [],
  "orientationSteps": 1, "rotation": 2.5,
  "saturation": 12, "shadows": 28, "sharpness": 30, "sharpnessThreshold": 25,
  "showClipping": 3, "structure": 10,
  "temperature": 8, "tint": -4, "toneMapper": "agx",
  "transformDistortion": 6, "transformVertical": -8, "transformHorizontal": 5, "transformRotate": 1.5,
  "transformAspect": 4, "transformScale": 95, "transformXOffset": 3, "transformYOffset": -2,
  "vibrance": 16,
  "vignetteAmount": -25, "vignetteCenterX": 0.45, "vignetteCenterY": 0.55, "vignetteFeather": 65,
  "vignetteMidpoint": 40, "vignetteRoundness": 15,
  "whiteBalance": { "temperature": 4300, "tint": 12 },
  "whiteBalanceAuto": { "temperature": 4300, "tint": 12, "photo": "/photos/2026/october/corpus-0001.raw" },
  "whites": 9,
  "cameraProfile": "Corpus Camera.dcp",
  "rawToneRendering": "baseCurve",
  "rawToneCurve": [
    { "x": 0, "y": 0 }, { "x": 64, "y": 52 }, { "x": 128, "y": 140 }, { "x": 192, "y": 214 },
    { "x": 255, "y": 255 }
  ],
  "rawToneCurveMode": "baseCurve",
  "agBlackWhite": {
    "enabled": true, "reds": 10, "oranges": 20, "yellows": -10, "greens": -20, "aquas": 5,
    "blues": -30, "purples": 15, "magentas": 25
  },
  "sectionVisibility": { "basic": true, "curves": true, "color": true, "details": true, "effects": false },
  "agSharpen": {
    "enabled": false, "capture": false, "captureAmount": 70, "autoRadius": false, "radius": 0.9,
    "cornerBoost": 0.2, "iterations": 30, "iterCheck": false, "amount": 60, "usmRadius": 1.4,
    "threshold": 2.5, "autoContrast": false, "contrast": 30, "usmOwnMask": true,
    "usmAutoContrast": false, "usmContrast": 25
  },
  "agRestore": { "captureRadius": 0.8 }
}"##;

/// A mask's adjustments: every key of INITIAL_MASK_ADJUSTMENTS (utils/adjustments.ts)
/// changed, with the mask's own section eyes and Sharpen.
const MASK_ADJUSTMENTS_JSON: &str = r##"{
  "activeTools": { "sharpening": false },
  "blacks": -5, "brightness": 0.2, "clarity": 10,
  "colorGrading": {
    "balance": 10, "blending": 40,
    "global": { "hue": 30, "saturation": 10, "luminance": 2 },
    "highlights": { "hue": 60, "saturation": 15, "luminance": -4 },
    "midtones": { "hue": 120, "saturation": 5, "luminance": 1 },
    "shadows": { "hue": 240, "saturation": 12, "luminance": 3 },
    "agLightroom": 1
  },
  "colorNoiseReduction": 15, "contrast": 12,
  "curves": {
    "blue": [{ "x": 0, "y": 0 }, { "x": 255, "y": 240 }],
    "green": [{ "x": 0, "y": 5 }, { "x": 255, "y": 255 }],
    "luma": [{ "x": 0, "y": 0 }, { "x": 100, "y": 110 }, { "x": 255, "y": 255 }],
    "red": [{ "x": 0, "y": 0 }, { "x": 200, "y": 190 }, { "x": 255, "y": 255 }]
  },
  "pointCurves": {
    "blue": [{ "x": 0, "y": 0 }, { "x": 255, "y": 248 }],
    "green": [{ "x": 0, "y": 2 }, { "x": 255, "y": 255 }],
    "luma": [{ "x": 0, "y": 0 }, { "x": 90, "y": 100 }, { "x": 255, "y": 255 }],
    "red": [{ "x": 0, "y": 0 }, { "x": 210, "y": 200 }, { "x": 255, "y": 255 }]
  },
  "parametricCurve": {
    "luma": { "darks": 5, "shadows": -4, "highlights": 3, "lights": -2, "whiteLevel": -3, "blackLevel": 2, "split1": 20, "split2": 50, "split3": 80 },
    "red": { "darks": 2, "shadows": 1, "highlights": -1, "lights": 1, "whiteLevel": -1, "blackLevel": 1, "split1": 25, "split2": 45, "split3": 75 },
    "green": { "darks": -1, "shadows": 2, "highlights": 1, "lights": -1, "whiteLevel": -2, "blackLevel": 1, "split1": 30, "split2": 50, "split3": 70 },
    "blue": { "darks": 3, "shadows": -2, "highlights": 2, "lights": -1, "whiteLevel": -1, "blackLevel": 3, "split1": 20, "split2": 40, "split3": 60 }
  },
  "curveMode": "parametric",
  "dehaze": 8, "exposure": 0.2,
  "flareAmount": 5, "glowAmount": 6, "halationAmount": 7,
  "highlights": -10,
  "hsl": {
    "aquas": { "hue": 2, "saturation": -4, "luminance": 1 },
    "blues": { "hue": -3, "saturation": 5, "luminance": -2 },
    "greens": { "hue": 4, "saturation": -6, "luminance": 2 },
    "magentas": { "hue": -1, "saturation": 3, "luminance": -1 },
    "oranges": { "hue": 1, "saturation": 4, "luminance": 3 },
    "purples": { "hue": 3, "saturation": -2, "luminance": 1 },
    "reds": { "hue": -2, "saturation": 6, "luminance": -1 },
    "yellows": { "hue": 1, "saturation": -3, "luminance": 2 }
  },
  "hue": 5, "lumaNoiseReduction": 10, "saturation": 15, "shadows": 12, "sharpness": 20,
  "sharpnessThreshold": 30, "structure": 6, "temperature": 10, "tint": -6, "vibrance": 9,
  "whites": 4,
  "sectionVisibility": { "basic": true, "curves": true, "color": true, "details": true, "effects": true },
  "agSharpen": { "amount": 40, "radius": 1.2, "threshold": 1.5, "autoContrast": false, "contrast": 20 }
}"##;
