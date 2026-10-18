//! North star 6: golden renders. Each tool, on each local RAW (`AG_RAW`,
//! `AG_RAWS`), rendered at preview size.
//!
//! Every case has to move the picture the way its slider says - brighter,
//! warmer, lifted shadows, greyer - measured on the pixels it acts on: the
//! shadows slider on the photo's darker tones, highlights on its brightest.
//! A photo with too few such pixels is not judged on that case. And every case is compared with the
//! render recorded the last time, in `AG_GOLDEN` (by default `golden` in the
//! target directory's scratch space): every channel of every pixel within
//! one 8-bit level. The first run on a machine records them and says so. A
//! render that differs is written beside its golden as `.new.bmp`; after a
//! change meant to move it, delete the golden to record it again.
//!
//! The goldens are BMPs, with a `sheet.html` beside them showing every case
//! of every photo side by side, to be judged by eye.

mod common;

use std::path::{Path, PathBuf};

use argentum_engine::colour::srgb_encode;
use argentum_engine::contract::EditPatch;
use argentum_engine::{Pipeline, Size, Source};

/// The long side of a golden render.
const SIZE_PX: u32 = 800;

/// A render on screen: 8-bit sRGB, row-major.
struct Shot {
    width: usize,
    height: usize,
    rgb: Vec<[u8; 3]>,
}

impl Shot {
    fn lumas(&self) -> Vec<f32> {
        self.rgb
            .iter()
            .map(|p| 0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32)
            .collect()
    }

    fn mean_luma(&self) -> f32 {
        let l = self.lumas();
        l.iter().sum::<f32>() / l.len() as f32
    }

    /// How far luminance moved, on average, over the pixels whose luminance
    /// in `opened` was in `lo..hi` levels: `None` when they are fewer than
    /// half a percent of the picture.
    fn shift(&self, opened: &Shot, lo: f32, hi: f32) -> Option<f32> {
        let (mut sum, mut n) = (0.0, 0usize);
        for (a, o) in self.lumas().iter().zip(opened.lumas()) {
            if (lo..hi).contains(&o) {
                sum += a - o;
                n += 1;
            }
        }
        (n * 200 >= self.rgb.len()).then(|| sum / n as f32)
    }

    /// Mean of each channel.
    fn means(&self) -> [f32; 3] {
        let n = self.rgb.len() as f32;
        [0, 1, 2].map(|c| self.rgb.iter().map(|p| p[c] as f32).sum::<f32>() / n)
    }

    /// Mean distance of a pixel's channels from their own mean.
    fn chroma(&self) -> f32 {
        let n = self.rgb.len() as f32;
        self.rgb
            .iter()
            .map(|p| {
                let m = (p[0] as f32 + p[1] as f32 + p[2] as f32) / 3.0;
                p.iter().map(|&v| (v as f32 - m).abs()).sum::<f32>()
            })
            .sum::<f32>()
            / n
    }
}

/// How a case has to have moved the picture, against the photo as opened:
/// `None` when the photo has too few of the pixels it acts on.
type Check = fn(&Shot, &Shot) -> Option<bool>;

struct Case {
    name: &'static str,
    /// The patch, given the photo's own white balance as it opens.
    patch: fn(f64, f64) -> serde_json::Value,
    moves: &'static str,
    check: Check,
}

fn cases() -> Vec<Case> {
    use serde_json::json;
    vec![
        Case {
            name: "exposure_plus_1ev",
            patch: |_, _| json!({ "ag_exposure": { "exposure_ev": 1.0 } }),
            moves: "brighter",
            check: |a, o| Some(a.mean_luma() > o.mean_luma() + 5.0),
        },
        Case {
            name: "white_balance_cooler",
            patch: |k, _| json!({ "rr_white_balance": { "temperature_k": k - 1500.0 } }),
            moves: "bluer",
            check: |a, o| Some(a.means()[2] / a.means()[0] > o.means()[2] / o.means()[0] * 1.03),
        },
        Case {
            name: "white_balance_warmer",
            patch: |k, _| json!({ "rr_white_balance": { "temperature_k": k + 2500.0 } }),
            moves: "redder",
            check: |a, o| Some(a.means()[0] / a.means()[2] > o.means()[0] / o.means()[2] * 1.03),
        },
        Case {
            name: "tint_plus_40",
            patch: |_, t| json!({ "rr_white_balance": { "tint": t + 40.0 } }),
            // A light said to be greener than shot is taken out: magenta,
            // as Lightroom's tint goes.
            moves: "more magenta",
            check: |a, o| {
                let green = |s: &Shot| s.means()[1] / (s.means()[0] + s.means()[2]);
                Some(green(a) < green(o) * 0.99)
            },
        },
        Case {
            name: "contrast_plus_60",
            patch: |_, _| json!({ "rr_basic_tone": { "contrast": 60.0 } }),
            // Around linear 0.22 before DNG Standard, which shows near
            // 190 of 255: the tones under it go down, those over it up.
            moves: "darker below, brighter above",
            check: |a, o| {
                let below = a.shift(o, 16.0, 128.0)?;
                let above = a.shift(o, 200.0, 250.0)?;
                Some(below < -2.0 && above > 0.5)
            },
        },
        Case {
            name: "highlights_minus_80",
            patch: |_, _| json!({ "rr_basic_tone": { "highlights": -80.0 } }),
            moves: "darker highlights",
            check: |a, o| Some(a.shift(o, 192.0, 256.0)? < -2.0),
        },
        Case {
            name: "shadows_plus_80",
            patch: |_, _| json!({ "rr_basic_tone": { "shadows": 80.0 } }),
            moves: "brighter shadows",
            check: |a, o| Some(a.shift(o, 16.0, 128.0)? > 2.0),
        },
        Case {
            name: "whites_plus_60",
            patch: |_, _| json!({ "rr_basic_tone": { "whites": 60.0 } }),
            moves: "brighter whites",
            // A small move on screen: DNG Standard's curve rolls the top off.
            check: |a, o| Some(a.shift(o, 192.0, 256.0)? > 0.1),
        },
        Case {
            name: "blacks_minus_60",
            patch: |_, _| json!({ "rr_basic_tone": { "blacks": -60.0 } }),
            moves: "darker blacks",
            check: |a, o| Some(a.shift(o, 16.0, 64.0)? < -2.0),
        },
        Case {
            name: "vibrance_plus_70",
            patch: |_, _| json!({ "rr_presence": { "vibrance": 70.0 } }),
            moves: "more colourful",
            check: |a, o| Some(a.chroma() > o.chroma() * 1.05),
        },
        Case {
            name: "saturation_minus_100",
            patch: |_, _| json!({ "rr_presence": { "saturation": -100.0 } }),
            moves: "grey",
            check: |a, _| Some(a.chroma() < 1.5),
        },
        Case {
            name: "dng_standard_off",
            patch: |_, _| json!({ "ag_dng_standard": { "enabled": false } }),
            moves: "darker mid-tones",
            check: |a, o| Some(a.mean_luma() < o.mean_luma() - 5.0),
        },
    ]
}

fn render(p: &mut Pipeline, source: &Source) -> Shot {
    let image = p.render(source, Size::Fit(SIZE_PX, SIZE_PX)).unwrap();
    let rgb = image.as_rgb().unwrap();
    Shot {
        width: rgb.width,
        height: rgb.height,
        rgb: rgb
            .pixels
            .iter()
            .map(|p| p.map(|v| (srgb_encode(v) * 255.0).round() as u8))
            .collect(),
    }
}

fn golden_dir() -> PathBuf {
    std::env::var_os("AG_GOLDEN")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("golden"))
}

/// A 24-bit BMP: what every browser and image viewer opens.
fn write_bmp(path: &Path, shot: &Shot) {
    let row = (shot.width * 3).div_ceil(4) * 4;
    let size = 54 + row * shot.height;
    let mut out = Vec::with_capacity(size);
    out.extend(b"BM");
    out.extend((size as u32).to_le_bytes());
    out.extend([0u8; 4]);
    out.extend(54u32.to_le_bytes());
    out.extend(40u32.to_le_bytes());
    out.extend((shot.width as i32).to_le_bytes());
    out.extend((shot.height as i32).to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(24u16.to_le_bytes());
    out.extend([0u8; 24]);
    for y in (0..shot.height).rev() {
        let start = out.len();
        for p in &shot.rgb[y * shot.width..(y + 1) * shot.width] {
            out.extend([p[2], p[1], p[0]]);
        }
        out.resize(start + row, 0);
    }
    std::fs::write(path, out).unwrap();
}

fn read_bmp(path: &Path) -> Option<Shot> {
    let bytes = std::fs::read(path).ok()?;
    let int = |at: usize| i32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    let (width, height) = (int(18) as usize, int(22) as usize);
    let row = (width * 3).div_ceil(4) * 4;
    let mut rgb = vec![[0u8; 3]; width * height];
    for y in 0..height {
        let line = &bytes[54 + (height - 1 - y) * row..];
        for x in 0..width {
            rgb[y * width + x] = [line[x * 3 + 2], line[x * 3 + 1], line[x * 3]];
        }
    }
    Some(Shot { width, height, rgb })
}

/// Against the golden: `None` when it was just recorded, else the largest
/// difference of any channel, in levels.
fn against_golden(path: &Path, shot: &Shot) -> Option<u8> {
    let Some(golden) = read_bmp(path) else {
        write_bmp(path, shot);
        return None;
    };
    let worst = if (golden.width, golden.height) != (shot.width, shot.height) {
        u8::MAX
    } else {
        golden
            .rgb
            .iter()
            .zip(&shot.rgb)
            .flat_map(|(a, b)| (0..3).map(move |c| a[c].abs_diff(b[c])))
            .max()
            .unwrap_or(0)
    };
    if worst > 1 {
        write_bmp(&path.with_extension("new.bmp"), shot);
    }
    Some(worst)
}

#[test]
fn every_tool_renders_as_it_did_and_moves_the_picture_its_way() {
    let raws = common::raws("every_tool_renders_as_it_did_and_moves_the_picture_its_way");
    // The per-pixel tools run on the GPU.
    if raws.is_empty()
        || common::gpu("every_tool_renders_as_it_did_and_moves_the_picture_its_way").is_none()
    {
        return;
    }
    let dir = golden_dir();
    let mut sheet = String::from(
        "<!doctype html><meta charset=utf-8><title>Golden renders</title><style>\
         body{background:#1b1b1b;color:#ddd;font:13px system-ui;margin:16px}\
         .row{display:flex;flex-wrap:wrap;gap:8px;margin:0 0 24px}figure{margin:0;width:300px}\
         img{width:100%;display:block}figcaption{padding:3px 0}</style>",
    );
    let mut failures = Vec::new();
    eprintln!("\n  goldens in {}", dir.display());
    for path in raws {
        let source = common::open(&path);
        let stem = path.file_stem().unwrap().to_string_lossy().to_string();
        let out = dir.join(&stem);
        std::fs::create_dir_all(&out).unwrap();
        let mut p = Pipeline::standard().unwrap();
        let defaults = p.registry().defaults_for(&source);
        let wb = &defaults.tools["rr_white_balance"].settings;
        let (kelvin, tint) = (
            wb["temperature_k"].as_f64().unwrap(),
            wb["tint"].as_f64().unwrap(),
        );
        p.set_edit(&defaults).unwrap();
        let opened = render(&mut p, &source);
        eprintln!("  {stem}: as shot {kelvin:.0} K, tint {tint:+.1}");
        sheet += &format!("<h3>{stem}</h3><div class=row>");
        let mut shots = vec![("as_opened", "", opened.rgb.clone(), Some(true))];
        for case in cases() {
            p.set_edit(&defaults).unwrap();
            let patch: EditPatch = serde_json::from_value((case.patch)(kelvin, tint)).unwrap();
            p.apply(&patch).unwrap();
            let shot = render(&mut p, &source);
            let moved = (case.check)(&shot, &opened);
            if moved == Some(false) {
                failures.push(format!("{stem}: {} is not {}", case.name, case.moves));
            }
            shots.push((case.name, case.moves, shot.rgb, moved));
        }
        for (name, moves, rgb, moved) in shots {
            let shot = Shot {
                width: opened.width,
                height: opened.height,
                rgb,
            };
            let file = out.join(format!("{name}.bmp"));
            let golden = against_golden(&file, &shot);
            let said = match golden {
                None => "recorded".to_string(),
                Some(worst) if worst <= 1 => format!("as before (worst {worst})"),
                Some(worst) => {
                    failures.push(format!(
                        "{stem}: {name} differs from its golden by {worst} levels"
                    ));
                    format!("CHANGED by {worst} levels")
                }
            };
            let moved = match moved {
                Some(true) => moves,
                Some(false) => "DID NOT MOVE",
                None => "not judged: too few pixels",
            };
            eprintln!("    {name:<22} {moved:<30} {said}");
            sheet += &format!(
                "<figure><img src='{stem}/{name}.bmp'><figcaption>{name} - {said}</figcaption></figure>"
            );
        }
        sheet += "</div>";
    }
    std::fs::write(dir.join("sheet.html"), sheet).unwrap();
    assert!(failures.is_empty(), "{failures:#?}");
}
