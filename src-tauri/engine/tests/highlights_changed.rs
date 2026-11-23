//! `dt_highlights` makes, to the bit, what the app's `mods/highlights.rs`
//! makes - the code it was copied from, compiled unchanged by the reference
//! crate - on inputs the parity test does not cover: float buffers, other
//! colour filter layouts, and a clipped photosite no block holds. And it says
//! it changed something exactly when the buffer differs from what it was
//! handed.

mod common;

use std::collections::HashMap;
use std::sync::Mutex;

use argentum_engine::rawler::cfa::{CFA, PlaneColor};
use argentum_engine::rawler::decoders::{Camera, Orientation};
use argentum_engine::rawler::rawimage::{
    BlackLevel, CFAConfig, RawImage, RawImageData, RawPhotometricInterpretation, WhiteLevel,
};
use argentum_engine::tools::dt_highlights::{self, DtHighlights};
use argentum_engine::{Context, GpuSlot, Image, Source, Tool};
use argentum_engine_reference::highlights as today;

/// The app's switch is one global, so the tests that flip it take turns.
static SWITCH: Mutex<()> = Mutex::new(());

/// The same kind of buffer holding the same bits.
fn same(a: &RawImageData, b: &RawImageData) -> bool {
    match (a, b) {
        (RawImageData::Integer(a), RawImageData::Integer(b)) => a == b,
        (RawImageData::Float(a), RawImageData::Float(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.to_bits() == y.to_bits())
        }
        _ => false,
    }
}

/// Run the app's code and ours on `raw`, recovery on and off; require the
/// same bits, and "changed" exactly when the buffer differs. Returns whether
/// anything changed with recovery on.
fn check(label: &str, raw: &RawImage) -> bool {
    let _turn = SWITCH.lock().unwrap_or_else(|e| e.into_inner());
    let mut changed_on = false;
    for recover in [true, false] {
        let mut theirs = raw.clone();
        today::set_enabled(recover);
        let ceilings = today::ceilings(&theirs);
        let rebuilt = today::recover(&mut theirs, ceilings);
        let wb = theirs.wb_coeffs;
        today::settle_blown(&mut theirs, wb, rebuilt, ceilings);
        today::set_enabled(true);
        let differs = !same(&raw.data, &theirs.data);

        let mut ours = raw.clone();
        let changed = dt_highlights::apply(&mut ours, recover);
        assert!(
            same(&ours.data, &theirs.data),
            "{label}, recovery {recover}: apply made something else"
        );
        assert_eq!(
            changed, differs,
            "{label}, recovery {recover}: apply's changed"
        );

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
        assert!(
            same(data, &theirs.data),
            "{label}, recovery {recover}: the tool made something else"
        );
        assert_eq!(
            out.is_some(),
            differs,
            "{label}, recovery {recover}: the tool's answer"
        );
        eprintln!("  {label:<48} recovery {recover:<5}  identical, changed {differs}");
        if recover {
            changed_on = differs;
        }
    }
    changed_on
}

fn with_cfa(mut raw: RawImage, pattern: &str) -> RawImage {
    let cfa = CFA::new(pattern);
    raw.photometric =
        RawPhotometricInterpretation::Cfa(CFAConfig::new(&cfa, &PlaneColor::new("RGB")));
    raw.camera.cfa = cfa;
    raw
}

/// The same values as floats, each scaled by `gain`.
fn float(mut raw: RawImage, gain: f32) -> RawImage {
    raw.data = RawImageData::Float(raw.data.as_f32().iter().map(|&x| x * gain).collect());
    raw
}

/// Canon sRAW's shape: three colours per pixel (as `parity_highlights.rs`).
fn three_colour(width: usize, height: usize) -> RawImage {
    let mosaic = common::scene(width * 3, height);
    let data: Vec<u16> = (0..height)
        .flat_map(|r| (0..width).map(move |c| (r, c)))
        .flat_map(|(r, c)| {
            let at = |dc: usize| mosaic.pixels_u16()[r * width * 3 + c * 3 + dc];
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

const XTRANS: &str = "GGRGGBGGBGGRBRGRBGGGBGGRGGRGGBRBGBRG";

#[test]
fn the_same_bits_and_changed_exactly_when_they_differ() {
    eprintln!();
    let dim = || common::bayer(258, 256, |r, c| 600 + ((r * 13 + c * 7) % 900) as u16);

    for (name, pattern) in [("RGGB", "RGGB"), ("BGGR", "BGGR"), ("X-Trans", XTRANS)] {
        let scene = with_cfa(common::scene(642, 480), pattern);
        assert!(check(&format!("{name}, a clipped disc"), &scene));
        assert!(check(
            &format!("{name}, a clipped disc, as floats"),
            &float(scene.clone(), 1.0)
        ));
        assert!(check(
            &format!("{name}, a clipped disc, as scaled floats"),
            &float(scene, 1.0003)
        ));
        let dim = with_cfa(dim(), pattern);
        assert!(!check(&format!("{name}, nothing clipped"), &dim));
        assert!(!check(
            &format!("{name}, nothing clipped, as floats"),
            &float(dim, 1.0)
        ));
    }

    let sraw = three_colour(320, 240);
    assert!(check("three-colour, a clipped disc", &sraw));
    assert!(check(
        "three-colour, a clipped disc, as floats",
        &float(sraw, 1.0)
    ));
    let dim3 = three_colour(32, 24);
    check("three-colour, small", &dim3);

    // Everything clipped, as integers and floats.
    let blown = common::bayer(128, 128, |_, _| 15000);
    assert!(check("all clipped", &blown));
    assert!(check("all clipped, as floats", &float(blown, 1.0)));

    // A few clipped photosites in a dim frame: too few to learn a colour from.
    let few = common::bayer(256, 256, |r, c| {
        if r % 64 == 0 && c % 64 == 0 {
            15000
        } else {
            600 + ((r * 13 + c * 7) % 900) as u16
        }
    });
    assert!(check("a few clipped photosites", &few));

    // Clipped only in a column no block holds: the scan sees it, nothing moves.
    let edge = common::bayer(129, 128, |_, c| if c == 128 { 15000 } else { 900 });
    assert!(!check("clipped only past the last block", &edge));
}
