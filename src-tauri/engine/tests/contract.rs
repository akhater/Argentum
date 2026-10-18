//! The interface's side of the contract is generated from `src/contract.rs`:
//!
//! - `src/shell/contract.generated.ts`, the TypeScript types;
//! - `src/shell/contract.fixtures.json`, one of each, made by the engine, for
//!   both sides to test against.
//!
//! This test fails when either file is not what `contract.rs` makes now. To
//! write them, in src-tauri/engine:
//!
//!     AG_WRITE_CONTRACT=1 cargo test --test contract

use std::path::PathBuf;

use argentum_engine::contract::{
    Applied, Choice, EDIT_VERSION, ENGINE_EVENT, EditDocument, EditPatch, EnabledWhen, EngineError,
    ErrorKind, Event, FRAME_HEADER_BYTES, FRAME_SHARED, FRAME_SHARED_KEY, HISTOGRAM_BINS, Opened,
    Origin, PhotoId, Placement, RenderTicket, Section, SectionDescriptor, SettingKind,
    SettingScale, SettingSpec, Stage, ToolDescriptor, ToolEdit, View, sections,
};
use argentum_engine::{Frame, placement, standard};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

fn shell() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../src/shell")
}

fn decl<T: TS>() -> String {
    format!("{}export {}\n", T::DOCS.unwrap_or(""), tidy(&T::decl()))
}

/// ts-rs writes an object type on one line, with its fields' doc comments
/// inline. One field to a line, indented, reads better.
fn tidy(decl: &str) -> String {
    let mut out = String::new();
    // The brackets open at this point. Only an object at the top level has
    // its fields split, and only between fields: `Record<string, X>` keeps
    // its comma.
    let mut open: Vec<char> = Vec::new();
    let mut rest = decl;
    while let Some(c) = rest.chars().next() {
        if rest.starts_with("/**") {
            // A doc comment, copied as it is: its text may hold brackets.
            let end = rest.find("*/").map_or(rest.len(), |i| i + 2);
            out.push_str(&rest[..end].replace('\n', "\n  "));
            rest = &rest[end..];
            continue;
        }
        rest = &rest[c.len_utf8()..];
        let fields = open == ['{'];
        match c {
            '{' if open.is_empty() => {
                open.push(c);
                out.push_str("{\n  ");
                rest = rest.strip_prefix(' ').unwrap_or(rest);
            }
            '}' if fields => {
                open.pop();
                out.truncate(out.trim_end().len());
                out.push_str("\n}");
            }
            ',' if fields => {
                out.push_str(",\n  ");
                rest = rest.strip_prefix(' ').unwrap_or(rest);
            }
            '\n' if fields => out.push_str("\n  "),
            '{' | '<' | '(' | '[' => {
                open.push(c);
                out.push(c);
            }
            '}' | '>' | ')' | ']' => {
                open.pop();
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out.lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn typescript() -> String {
    let mut out = String::from(
        "// Generated from src-tauri/engine/src/contract.rs by\n\
         // src-tauri/engine/tests/contract.rs. Do not edit: change contract.rs, then\n\
         // run `AG_WRITE_CONTRACT=1 cargo test --test contract` in src-tauri/engine.\n\
         \n\
         /** A setting's value: a number, a boolean, or the value of a choice. */\n\
         export type SettingValue = number | boolean | string;\n\
         \n",
    );
    out += &format!("export const EDIT_VERSION = {EDIT_VERSION};\n\n");
    out += &format!("export const HISTOGRAM_BINS = {HISTOGRAM_BINS};\n\n");
    out += &format!("export const ENGINE_EVENT = \"{ENGINE_EVENT}\";\n\n");
    out += &format!("export const FRAME_HEADER_BYTES = {FRAME_HEADER_BYTES};\n\n");
    out += &format!("export const FRAME_SHARED = \"{FRAME_SHARED}\";\n\n");
    out += &format!("export const FRAME_SHARED_KEY = \"{FRAME_SHARED_KEY}\";\n\n");
    for d in [
        decl::<Origin>(),
        decl::<Stage>(),
        decl::<Section>(),
        decl::<SectionDescriptor>(),
        decl::<SettingKind>(),
        decl::<SettingScale>(),
        decl::<Choice>(),
        decl::<EnabledWhen>(),
        decl::<SettingSpec>(),
        decl::<ToolDescriptor>(),
        decl::<ToolEdit>(),
        decl::<EditDocument>(),
        decl::<EditPatch>(),
        decl::<PhotoId>(),
        decl::<RenderTicket>(),
        decl::<Applied>(),
        decl::<Opened>(),
        decl::<View>(),
        decl::<Placement>(),
        decl::<ErrorKind>(),
        decl::<EngineError>(),
        decl::<Event>(),
    ] {
        out += &d;
        out += "\n";
    }
    out.truncate(out.trim_end().len());
    out.push('\n');
    out
}

/// One of each, made by the engine itself.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
struct Fixtures {
    sections: Vec<SectionDescriptor>,
    tools: Vec<ToolDescriptor>,
    edit: EditDocument,
    patch: EditPatch,
    applied: Applied,
    /// Patches the engine takes, each applied to `edit`, and what it made.
    patched: Vec<Patched>,
    /// Patches the engine refuses, each applied to `edit`, and why.
    refused: Vec<Refused>,
    opened: Opened,
    view: View,
    /// Where the frame of each view goes, as the engine places it.
    placements: Vec<Placed>,
    /// A frame as it crosses to the interface, made by the engine.
    frame_block: FrameBlock,
    events: Vec<Event>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
struct Placed {
    view: View,
    photo_width_px: u32,
    photo_height_px: u32,
    placement: Placement,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
struct FrameBlock {
    ticket: RenderTicket,
    width_px: u32,
    height_px: u32,
    rgba: Vec<u8>,
    bytes: Vec<u8>,
}

/// Views worth pinning: fitted with bands, at 100% filling the view, past
/// 100% at a corner, and a photo smaller than its view.
fn placements() -> Vec<Placed> {
    let views = [
        (1600.0 / 7144.0, 0.5, 0.5, 1600, 1200, 7144, 4760),
        (1.0, 0.5, 0.5, 1786, 1190, 7144, 4760),
        (2.0, 0.0, 0.0, 800, 600, 7144, 4760),
        (1.0, 0.5, 0.5, 800, 600, 640, 480),
    ];
    views
        .into_iter()
        .map(
            |(zoom, center_x, center_y, width, height, photo_w, photo_h)| {
                let view = View {
                    zoom,
                    center_x,
                    center_y,
                    width,
                    height,
                };
                Placed {
                    placement: placement(&view, photo_w, photo_h),
                    view,
                    photo_width_px: photo_w,
                    photo_height_px: photo_h,
                }
            },
        )
        .collect()
}

fn frame_block() -> FrameBlock {
    let frame = Frame {
        ticket: RenderTicket(7),
        placement: Placement {
            x_px: 0,
            y_px: 0,
            width_px: 2,
            height_px: 1,
        },
        rgba: vec![255, 0, 0, 255, 0, 128, 255, 255],
    };
    FrameBlock {
        ticket: frame.ticket,
        width_px: frame.placement.width_px,
        height_px: frame.placement.height_px,
        bytes: frame.block(),
        rgba: frame.rgba,
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct Patched {
    patch: EditPatch,
    edit: EditDocument,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct Refused {
    patch: EditPatch,
    error: EngineError,
}

fn patch(json: serde_json::Value) -> EditPatch {
    serde_json::from_value(json).unwrap()
}

/// What the engine does with each of these, as it does it: clamping,
/// defaults, refusals.
fn patched_and_refused(edit: &EditDocument) -> (Vec<Patched>, Vec<Refused>) {
    let registry = standard();
    let taken = [
        // As given.
        serde_json::json!({ "ag_exposure_wb": { "exposure": 0.5, "red": 1.05 } }),
        // An integer for a float.
        serde_json::json!({ "ag_exposure_wb": { "exposure": 1 } }),
        // Out of range: clamped, not refused.
        serde_json::json!({
            "ag_exposure_wb": { "exposure": -20 },
            "rt_capture_sharpen": { "radius": 9, "iterations": 500 },
        }),
        // null is the default.
        serde_json::json!({ "ag_exposure_wb": { "exposure": null, "blue": 2 } }),
    ];
    let refused = [
        // A tool or a setting this engine does not have.
        serde_json::json!({ "ag_nothing": { "x": 1 } }),
        serde_json::json!({ "ag_exposure_wb": { "contrast": 1 } }),
        // Wrong types: refused, never rounded or coerced.
        serde_json::json!({ "ag_exposure_wb": { "exposure": "bright" } }),
        serde_json::json!({ "rt_capture_sharpen": { "iterations": 2.5 } }),
        serde_json::json!({ "dt_highlights": { "recover": 1 } }),
        // All or nothing: one bad tool refuses the whole patch.
        serde_json::json!({
            "ag_exposure_wb": { "exposure": 1 },
            "dt_highlights": { "recover": "yes" },
        }),
    ];
    let taken = taken
        .into_iter()
        .map(|json| {
            let patch = patch(json);
            let edit = registry.merge(edit, &patch).expect("taken");
            Patched { patch, edit }
        })
        .collect();
    let refused = refused
        .into_iter()
        .map(|json| {
            let patch = patch(json);
            let error = registry.merge(edit, &patch).expect_err("refused");
            Refused {
                patch,
                error: error.to_contract(),
            }
        })
        .collect();
    (taken, refused)
}

fn fixtures() -> Fixtures {
    let edit = standard().default_edit();
    let patch = patch(serde_json::json!({
        "ag_exposure_wb": { "exposure": 0.5, "red": 1.05 },
        "rt_capture_sharpen": { "radius": null },
    }));
    let (patched, refused) = patched_and_refused(&edit);
    let mut bins = vec![0u32; HISTOGRAM_BINS];
    bins[118] = 1_000_000;
    bins[255] = 500_000;
    Fixtures {
        sections: sections(),
        tools: standard().descriptors(),
        opened: Opened {
            photo_id: PhotoId(1),
            width: 7144,
            height: 4760,
            edit: edit.clone(),
        },
        applied: Applied {
            ticket: RenderTicket(2),
            edit: patched[0].edit.clone(),
        },
        patched,
        refused,
        placements: placements(),
        frame_block: frame_block(),
        edit,
        patch,
        view: View {
            zoom: 0.25,
            center_x: 0.5,
            center_y: 0.5,
            width: 1786,
            height: 1190,
        },
        events: vec![
            Event::Frame {
                ticket: RenderTicket(3),
                photo_id: PhotoId(1),
                placement: placements()[0].placement,
            },
            Event::Histogram {
                ticket: RenderTicket(3),
                photo_id: PhotoId(1),
                bins,
            },
            Event::Error {
                ticket: Some(RenderTicket(4)),
                kind: ErrorKind::NoGpu,
                message: "no GPU: no adapter".into(),
            },
        ],
    }
}

fn fixtures_json() -> String {
    let mut text = serde_json::to_string_pretty(&fixtures()).unwrap();
    text.push('\n');
    text
}

/// Read with Windows line endings taken out, so a checkout that converts
/// them is not stale.
fn read(path: &PathBuf) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.replace("\r\n", "\n"))
}

#[test]
fn the_generated_files_are_current() {
    let files = [
        (shell().join("contract.generated.ts"), typescript()),
        (shell().join("contract.fixtures.json"), fixtures_json()),
    ];
    if std::env::var_os("AG_WRITE_CONTRACT").is_some() {
        std::fs::create_dir_all(shell()).unwrap();
        for (path, text) in &files {
            std::fs::write(path, text).unwrap();
            eprintln!("wrote {}", path.display());
        }
        return;
    }
    for (path, text) in &files {
        assert!(
            read(path).as_deref() == Some(text.as_str()),
            "{} is stale: contract.rs now generates something else. \
             Run `AG_WRITE_CONTRACT=1 cargo test --test contract` in src-tauri/engine \
             and commit the result.",
            path.display()
        );
    }
}

#[test]
fn what_the_engine_takes_and_refuses() {
    let (patched, refused) = patched_and_refused(&standard().default_edit());
    let kinds: Vec<_> = refused.iter().map(|r| r.error.kind).collect();
    assert_eq!(
        kinds,
        [
            ErrorKind::UnknownTool,
            ErrorKind::UnknownSetting,
            ErrorKind::BadValue,
            ErrorKind::BadValue,
            ErrorKind::BadValue,
            ErrorKind::BadValue,
        ]
    );
    let clamped = &patched[2].edit.tools;
    assert_eq!(clamped["ag_exposure_wb"].settings["exposure"], -10.0);
    assert_eq!(clamped["rt_capture_sharpen"].settings["radius"], 2.0);
    assert_eq!(clamped["rt_capture_sharpen"].settings["iterations"], 100);
    let reset = &patched[3].edit.tools["ag_exposure_wb"].settings;
    assert_eq!(
        (&reset["exposure"], &reset["blue"]),
        (&serde_json::json!(0.0), &serde_json::json!(2.0))
    );
}

#[test]
fn the_fixtures_read_back_as_the_contract() {
    let text = fixtures_json();
    let back: Fixtures = serde_json::from_str(&text).unwrap();
    assert_eq!(back, fixtures());
}

#[test]
fn the_wire_names_are_the_contracts() {
    let event = serde_json::to_value(Event::Frame {
        ticket: RenderTicket(7),
        photo_id: PhotoId(2),
        placement: Placement {
            x_px: 1,
            y_px: 2,
            width_px: 10,
            height_px: 20,
        },
    })
    .unwrap();
    assert_eq!(
        event,
        serde_json::json!({
            "event": "frame",
            "ticket": 7,
            "photoId": 2,
            "placement": { "xPx": 1, "yPx": 2, "widthPx": 10, "heightPx": 20 },
        })
    );
    let radius = &fixtures().tools[1].settings[3];
    assert_eq!(radius.key, "radius");
    let spec = serde_json::to_value(radius).unwrap();
    assert_eq!(
        spec["enabledWhen"],
        serde_json::json!({ "key": "autoRadius", "equals": false })
    );
    assert_eq!(spec["scale"], "linear");
    let opened = serde_json::to_value(&fixtures().opened).unwrap();
    assert!(opened.get("photoId").is_some());
    let view = serde_json::to_value(fixtures().view).unwrap();
    assert!(view.get("centerX").is_some());
    let stage = serde_json::to_value(Stage::SceneLinear).unwrap();
    assert_eq!(stage, "scene");
    let origin = serde_json::to_value(Origin::RawTherapee).unwrap();
    assert_eq!(origin, "rt");
}
