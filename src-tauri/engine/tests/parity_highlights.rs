//! Pass test 3a: `dt_highlights` is bit-identical to the app's
//! `mods/highlights.rs` on the same input.
//!
//! The app's side is its own file, compiled unchanged by the reference crate,
//! and run the way the app's decode runs it: `recover`, then `settle_blown`
//! with the file's white balance, with recovery on and off.

mod common;

use std::collections::HashMap;
use std::sync::Mutex;

use argentum_engine::rawler::decoders::{Camera, Orientation};
use argentum_engine::rawler::rawimage::{
    BlackLevel, RawImage, RawImageData, RawPhotometricInterpretation, WhiteLevel,
};
use argentum_engine::tools::dt_highlights::{self, DtHighlights};
use argentum_engine::{Context, GpuSlot, Image, Source, Tool};
use argentum_engine_reference::highlights as today;

/// The app's switch is one global, so the tests that flip it take turns.
static SWITCH: Mutex<()> = Mutex::new(());

/// Every value of the two buffers, compared as bits.
fn identical(ours: &RawImageData, theirs: &RawImageData) -> Result<(), String> {
    match (ours, theirs) {
        (RawImageData::Integer(a), RawImageData::Integer(b)) => {
            if a == b {
                Ok(())
            } else {
                Err("integer buffers differ".into())
            }
        }
        (RawImageData::Float(a), RawImageData::Float(b)) => {
            if a.len() != b.len() {
                return Err("float buffers of different lengths".into());
            }
            match a
                .iter()
                .zip(b)
                .position(|(x, y)| x.to_bits() != y.to_bits())
            {
                None => Ok(()),
                Some(i) => Err(format!("value {i}: ours {} theirs {}", a[i], b[i])),
            }
        }
        _ => Err("one is integer, the other float".into()),
    }
}

/// How many values the app's recovery moved.
fn moved(before: &RawImageData, after: &RawImageData) -> usize {
    let a = before.as_f32();
    let b = after.as_f32();
    a.iter().zip(b.iter()).filter(|(x, y)| x != y).count()
}

/// Run both on `raw`, both ways, and require the same bits. Returns how many
/// values recovery-on moved.
fn compare(label: &str, raw: &RawImage) -> usize {
    assert!(
        std::env::var_os("AG_NO_RECOVERY").is_none(),
        "AG_NO_RECOVERY is set, which switches the app's recovery off: unset it"
    );
    let _turn = SWITCH.lock().unwrap_or_else(|e| e.into_inner());
    let mut moved_on = 0;
    for recover in [true, false] {
        let mut theirs = raw.clone();
        today::set_enabled(recover);
        let rebuilt = today::recover(&mut theirs);
        let wb = theirs.wb_coeffs;
        today::settle_blown(&mut theirs, wb, rebuilt);
        today::set_enabled(true);

        let mut ours = raw.clone();
        dt_highlights::apply(&mut ours, recover);
        if let Err(e) = identical(&ours.data, &theirs.data) {
            panic!("{label}, recovery {recover}: {e}");
        }

        // And through the tool, as the pipeline calls it.
        let source = Source::new(1, raw.clone());
        let slot = GpuSlot::new();
        let cx = Context::new::<DtHighlights>(&source, 1, &slot);
        let out = DtHighlights
            .process(
                &dt_highlights::Settings { recover },
                &Image::Raw(source.raw.clone()),
                &cx,
            )
            .unwrap();
        let data = match &out {
            Some(Image::Raw(r)) => &r.data,
            Some(Image::Rgb(_)) => panic!("{label}: the tool made RGB"),
            None => &raw.data,
        };
        if let Err(e) = identical(data, &theirs.data) {
            panic!("{label}, recovery {recover}, through the tool: {e}");
        }
        if out.is_none() {
            assert!(
                matches!(theirs.data, RawImageData::Integer(_)),
                "{label}: the tool said nothing changed and the app changed something"
            );
        }

        let n = moved(&raw.data, &theirs.data);
        eprintln!(
            "  {label:<52} recovery {:<5}  identical, {n} of {} values moved",
            recover,
            raw.data.as_f32().len()
        );
        if recover {
            moved_on = n;
        }
    }
    moved_on
}

/// Canon sRAW's shape: three colours per pixel.
fn three_colour(width: usize, height: usize) -> RawImage {
    let mosaic = common::scene(width * 3, height);
    let data: Vec<u16> = (0..height)
        .flat_map(|r| (0..width).map(move |c| (r, c)))
        .flat_map(|(r, c)| {
            let at = |dc: usize| mosaic.pixels_u16()[r * width * 3 + c * 3 + dc];
            // The scene's red, green and blue sensitivities.
            [at(0), at(1).max(at(0)), at(2)]
        })
        .collect();
    RawImage {
        camera: Camera::default(),
        make: "Synthetic".into(),
        model: "Three colour".into(),
        clean_make: "Synthetic".into(),
        clean_model: "Three colour".into(),
        width,
        height,
        cpp: 3,
        bps: 16,
        wb_coeffs: [2.0, 1.0, 1.5, f32::NAN],
        whitelevel: WhiteLevel::new(vec![15000, 15000, 15000]),
        blacklevel: BlackLevel::new(&[0u32, 0, 0], 1, 1, 3),
        xyz_to_cam: [[0.0; 3]; 4],
        photometric: RawPhotometricInterpretation::LinearRaw,
        active_area: None,
        crop_area: None,
        blackareas: Vec::new(),
        orientation: Orientation::Normal,
        data: RawImageData::Integer(data),
        color_matrix: HashMap::new(),
        dng_tags: HashMap::new(),
    }
}

#[test]
fn bit_identical_on_synthetic_sensor_data() {
    eprintln!();
    let bayer = common::scene(640, 480);
    assert!(compare("synthetic Bayer, a clipped disc", &bayer) > 0);

    let sraw = three_colour(320, 240);
    assert!(compare("synthetic three-colour, a clipped disc", &sraw) > 0);

    // Nothing clipped: untouched, and the integers kept.
    let dim = common::bayer(256, 256, |r, c| 600 + ((r * 13 + c * 7) % 900) as u16);
    assert_eq!(compare("synthetic Bayer, nothing clipped", &dim), 0);

    // Everything clipped: nothing to learn a colour from.
    let blown = common::bayer(128, 128, |_, _| 15000);
    compare("synthetic Bayer, all clipped", &blown);
}

#[test]
fn bit_identical_on_real_raws() {
    let paths = common::raws("bit_identical_on_real_raws");
    eprintln!();
    for path in paths {
        let source = common::open(&path);
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        compare(&name, &source.raw);
    }
}
