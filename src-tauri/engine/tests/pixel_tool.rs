//! A per-pixel tool is one file and one registry line, and the pixel pass
//! takes it from the registry.
//!
//! The tool is `support/ag_tint.rs`; its registry line is the `.with` in
//! `registry()` below. No file of the engine names it, yet it runs in the
//! same pass as the standard per-pixel tools, called after them in registry
//! order, in renders and in a view's frame alike.

#[path = "support/ag_tint.rs"]
mod ag_tint;
mod common;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use ag_tint::AgTint;
use argentum_engine::contract::{EditPatch, Event, View};
use argentum_engine::{Engine, GpuSlot, Pipeline, Registry, Section, Size, standard};

fn registry() -> Registry {
    standard().with::<AgTint>()
}

fn patch(json: serde_json::Value) -> EditPatch {
    serde_json::from_value(json).unwrap()
}

#[test]
fn it_runs_in_the_pass_after_the_tools_before_it() {
    let Some(gpu) = common::gpu("it_runs_in_the_pass_after_the_tools_before_it") else {
        return;
    };
    let source = common::source(256, 192);
    let mut p = Pipeline::with_gpu(Arc::new(registry()), GpuSlot::with(gpu)).unwrap();
    p.apply(&patch(serde_json::json!({
        "rt_capture_sharpen": { "capture": false },
        "rr_presence": { "saturation": 20 },
    })))
    .unwrap();
    let before = p.render(&source, Size::Full).unwrap();
    p.apply(&patch(
        serde_json::json!({ "ag_tint": { "red": 0.5, "blue": -0.5 } }),
    ))
    .unwrap();
    let after = p.render(&source, Size::Full).unwrap();
    // One step for every per-pixel tool, the new one with them.
    let ran: Vec<_> = p
        .last_render()
        .iter()
        .filter(|s| s.ran)
        .map(|s| s.name)
        .collect();
    assert_eq!(ran, ["pixels"]);
    // Called last, on what the standard tools made.
    for (a, b) in after
        .as_rgb()
        .unwrap()
        .pixels
        .iter()
        .zip(&before.as_rgb().unwrap().pixels)
    {
        let want = [b[0] * 1.5, b[1], b[2] * 0.5];
        for c in 0..3 {
            assert!(
                (a[c] - want[c]).abs() <= want[c].abs() * 1e-6 + 1e-7,
                "{a:?} vs {want:?}"
            );
        }
    }
}

#[test]
fn a_frame_has_it_too() {
    let Some(_) = common::gpu("a_frame_has_it_too") else {
        return;
    };
    let (tx, rx) = std::sync::mpsc::channel();
    let engine = Engine::with_registry(registry(), move |e| {
        let _ = tx.send(e);
    })
    .unwrap();
    let id = engine.insert(common::source(256, 192)).photo_id;
    let frame = |json| {
        let ticket = engine.apply(id, &patch(json)).unwrap().ticket;
        loop {
            match rx.recv_timeout(Duration::from_secs(60)).expect("an event") {
                Event::Frame { ticket: t, .. } if t == ticket => break,
                Event::Error { message, .. } => panic!("{message}"),
                _ => {}
            }
        }
        engine.frame(ticket).unwrap()
    };
    let plain = frame(serde_json::json!({ "rt_capture_sharpen": { "capture": false } }));
    let tinted = frame(serde_json::json!({ "ag_tint": { "red": 0.3, "blue": -0.3 } }));
    let mean = |rgba: &[u8], c: usize| {
        rgba.as_chunks::<4>()
            .0
            .iter()
            .map(|p| p[c] as f64)
            .sum::<f64>()
            / (rgba.len() / 4) as f64
    };
    assert!(mean(&tinted.rgba, 0) > mean(&plain.rgba, 0) + 1.0);
    assert!(mean(&tinted.rgba, 2) < mean(&plain.rgba, 2) - 1.0);
    // And at 200%, each photo pixel two screen pixels across.
    let view = View {
        zoom: 2.0,
        center_x: 0.0,
        center_y: 0.0,
        width: 64,
        height: 64,
    };
    let ticket = engine.view(id, view).unwrap();
    loop {
        match rx.recv_timeout(Duration::from_secs(60)).expect("an event") {
            Event::Frame { ticket: t, .. } if t == ticket => break,
            Event::Error { message, .. } => panic!("{message}"),
            _ => {}
        }
    }
    let zoomed = engine.frame(ticket).unwrap();
    assert_eq!(zoomed.rgba[0..4], zoomed.rgba[4..8]);
}

#[test]
fn the_interface_is_told_about_it() {
    let engine = Engine::with_registry(registry(), |_| {}).unwrap();
    let last = engine.tools().pop().unwrap();
    assert_eq!(
        (last.id.as_str(), last.section),
        ("ag_tint", Section::Color)
    );
}

/// The pass took the tool from the registry: no file of the engine's own
/// names it.
#[test]
fn no_file_of_the_engine_names_it() {
    fn files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                files(&path, out);
            } else {
                out.push(path);
            }
        }
    }
    let mut all = Vec::new();
    files(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut all);
    assert!(all.len() > 10, "found the engine's source");
    for path in all {
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(
            !text.contains("ag_tint"),
            "{} names the test's tool",
            path.display()
        );
    }
}
