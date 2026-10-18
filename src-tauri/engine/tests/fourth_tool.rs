//! Pass test 1: adding a fourth tool is one new file and one registry line.
//!
//! The tool is `support/ag_dummy.rs`. The registry line is the `.with` in
//! `registry()` below - the same call each line of `registry!` in
//! src/registry.rs expands to. Everything else here uses the engine as it
//! is: the tool is described to the interface, its saved settings are read,
//! migrated and clamped, patches reach it, it runs in order, and its result
//! is cached and invalidated like every other tool's.

#[path = "support/ag_dummy.rs"]
mod ag_dummy;
mod common;

use ag_dummy::AgDummy;
use argentum_engine::contract::{EditPatch, SettingKind, ToolEdit};
use argentum_engine::tools::{ag_dng_standard, rt_capture_sharpen};
use argentum_engine::{Engine, Origin, Pipeline, Registry, Section, Size, Stage, standard};

fn registry() -> Registry {
    standard().with::<AgDummy>()
}

fn ran(p: &Pipeline) -> Vec<&'static str> {
    p.last_render()
        .iter()
        .filter(|s| s.ran)
        .map(|s| s.name)
        .collect()
}

fn patch(json: serde_json::Value) -> EditPatch {
    serde_json::from_value(json).unwrap()
}

fn pipeline() -> Pipeline {
    let mut p = Pipeline::new(registry()).expect("a valid registry");
    // No GPU needed for this test: nothing that runs on one has anything to
    // do. The per-pixel tools are `tests/pixel_tool.rs`'s.
    p.set::<rt_capture_sharpen::RtCaptureSharpen>(rt_capture_sharpen::Settings {
        capture: false,
        ..Default::default()
    })
    .unwrap();
    p.set::<ag_dng_standard::AgDngStandard>(ag_dng_standard::Settings {
        enabled: false,
        ..Default::default()
    })
    .unwrap();
    p
}

#[test]
fn a_fourth_tool_is_one_file_and_one_registry_line() {
    let mut p = pipeline();

    // Described last, with everything a panel needs, read off the tool.
    let tools = p.registry().descriptors();
    let standard = argentum_engine::standard().descriptors().len();
    assert_eq!(tools.len(), standard + 1);
    let d = tools.last().unwrap();
    assert_eq!(d.id, "ag_dummy");
    assert_eq!(d.origin, Origin::Argentum);
    assert_eq!(d.label, "tools.ag_dummy.label");
    assert_eq!(
        (d.section, d.order, d.stage),
        (Section::Effects, standard as u32, Stage::SceneLinear)
    );
    assert_eq!(d.version, 2);
    let [amount] = &d.settings[..] else {
        panic!("one setting: {:?}", d.settings)
    };
    assert_eq!(amount.kind, SettingKind::Float);
    assert_eq!((amount.min, amount.max), (Some(0.0), Some(1.0)));
    assert_eq!(amount.default, serde_json::json!(0.0));

    // An edit saved by its version 1 is migrated, and clamped to its range.
    p.set_json("ag_dummy", 1, serde_json::json!({ "strength": 50 }))
        .unwrap();
    assert_eq!(p.get::<AgDummy>().unwrap().amount, 0.5);
    p.apply(&patch(serde_json::json!({ "ag_dummy": { "amount": 7 } })))
        .unwrap();
    assert_eq!(p.get::<AgDummy>().unwrap().amount, 1.0);
    assert!(p.set_json("ag_dummy", 3, serde_json::json!({})).is_err());
    p.apply(&patch(
        serde_json::json!({ "ag_dummy": { "amount": 0.25 } }),
    ))
    .unwrap();

    // And saved in the edit at its own version.
    assert_eq!(
        p.edit().tools["ag_dummy"],
        ToolEdit {
            v: 2,
            settings: [("amount".to_string(), serde_json::json!(0.25))].into(),
        }
    );

    // It runs after every other tool, once.
    let source = common::source(256, 192);
    let out = p.render(&source, Size::Full).unwrap();
    assert_eq!(
        ran(&p),
        [
            "dt_highlights",
            "develop",
            "rt_capture_sharpen",
            "pixels",
            "ag_dummy"
        ]
    );

    // Its result is what it does to the tool before it.
    let mut without = pipeline();
    let before = without.render(&source, Size::Full).unwrap();
    let (a, b) = (before.as_rgb().unwrap(), out.as_rgb().unwrap());
    for (x, y) in a.pixels.iter().zip(&b.pixels) {
        for c in 0..3 {
            assert_eq!(y[c], x[c] + (ag_dummy::GREY - x[c]) * 0.25);
        }
    }

    // Cached like the others: nothing runs again, a patch to it runs it
    // alone, and a patch to a tool before it runs that tool and it.
    p.render(&source, Size::Full).unwrap();
    assert!(ran(&p).is_empty(), "{:?}", ran(&p));

    p.apply(&patch(
        serde_json::json!({ "ag_dummy": { "amount": 0.75 } }),
    ))
    .unwrap();
    p.render(&source, Size::Full).unwrap();
    assert_eq!(ran(&p), ["ag_dummy"]);

    p.apply(&patch(
        serde_json::json!({ "ag_dng_standard": { "baseline_exposure_ev": 0.6 } }),
    ))
    .unwrap();
    p.render(&source, Size::Full).unwrap();
    assert_eq!(ran(&p), ["pixels", "ag_dummy"]);
    assert_eq!(p.runs("ag_dummy"), 3);
}

#[test]
fn the_interface_is_told_about_it_too() {
    let engine = Engine::with_registry(registry(), |_| {}).unwrap();
    let ids: Vec<_> = engine.tools().into_iter().map(|d| d.id).collect();
    assert_eq!(
        ids,
        [
            "dt_highlights",
            "rt_capture_sharpen",
            "ag_exposure",
            "rr_white_balance",
            "rr_basic_tone",
            "rr_presence",
            "ag_dng_standard",
            "ag_dummy"
        ]
    );
}

#[test]
fn the_standard_registry_does_not_know_it() {
    let p = Pipeline::standard().unwrap();
    assert!(p.registry().position("ag_dummy").is_none());
    assert!(p.get::<AgDummy>().is_none());
}
