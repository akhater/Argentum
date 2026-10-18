//! The engine as the interface uses it: open, apply, view, and the events.

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use argentum_engine::contract::{
    EditPatch, ErrorKind, Event, PhotoId, Placement, RenderTicket, View,
};
use argentum_engine::rawler::imgop::{Dim2, Point, Rect};
use argentum_engine::{
    Context, Engine, Error, Image, Pipeline, Registry, Result, Section, Size, Source, Stage, Tool,
    standard,
};
use serde::{Deserialize, Serialize};

fn patch(json: serde_json::Value) -> EditPatch {
    serde_json::from_value(json).unwrap()
}

fn engine() -> (Engine, Receiver<Event>) {
    engine_with(standard)
}

fn engine_with(registry: fn() -> Registry) -> (Engine, Receiver<Event>) {
    let (tx, rx) = channel();
    let engine = Engine::with_registry(registry(), move |e| {
        let _ = tx.send(e);
    })
    .unwrap();
    (engine, rx)
}

/// Events until `ticket`'s frame and histogram, all of them, in order.
fn until_done(rx: &Receiver<Event>, ticket: RenderTicket) -> Vec<Event> {
    let mut seen = Vec::new();
    loop {
        let event = rx
            .recv_timeout(Duration::from_secs(60))
            .unwrap_or_else(|_| panic!("no histogram for {ticket:?}; saw {seen:?}"));
        let done = matches!(&event, Event::Histogram { ticket: t, .. } if *t == ticket)
            || matches!(&event, Event::Error { ticket: Some(t), .. } if *t == ticket);
        seen.push(event);
        if done {
            return seen;
        }
    }
}

fn no_sharpening() -> EditPatch {
    patch(serde_json::json!({ "rt_capture_sharpen": { "capture": false } }))
}

#[test]
fn open_apply_view_and_what_comes_back() {
    let (engine, rx) = engine();
    let opened = engine.insert(common::source(512, 384));
    assert_eq!((opened.width, opened.height), (512, 384));
    assert_eq!(
        opened.edit,
        Pipeline::standard().unwrap().edit(),
        "all defaults"
    );
    assert_eq!(engine.tools().len(), 3);
    let order: Vec<_> = engine.sections().iter().map(|s| s.label.clone()).collect();
    assert_eq!(
        order,
        [
            "sections.basic",
            "sections.tone",
            "sections.color",
            "sections.detail",
            "sections.lens",
            "sections.effects"
        ]
    );

    // The first view is the whole photo, fitted in 1600 px: here, all of it.
    let applied = engine.apply(opened.photo_id, &no_sharpening()).unwrap();
    let t1 = applied.ticket;
    // The edit as the engine took it: the patch in, everything else default.
    assert_eq!(
        applied.edit.tools["rt_capture_sharpen"].settings["capture"],
        serde_json::json!(false)
    );
    assert_eq!(
        applied.edit.tools["ag_exposure_wb"],
        opened.edit.tools["ag_exposure_wb"]
    );
    let events = until_done(&rx, t1);
    let [
        Event::Frame {
            ticket,
            photo_id,
            placement,
        },
        Event::Histogram {
            ticket: t,
            photo_id: p,
            bins,
        },
    ] = &events[..]
    else {
        panic!("{events:?}")
    };
    assert_eq!((*ticket, *t), (t1, t1));
    assert_eq!((*photo_id, *p), (opened.photo_id, opened.photo_id));
    // The first view fits the photo in 1600 px: here, all of it, filling it.
    assert_eq!(
        *placement,
        Placement {
            x_px: 0,
            y_px: 0,
            width_px: 512,
            height_px: 384
        }
    );
    assert_eq!(bins.len(), 256);
    assert_eq!(bins.iter().sum::<u32>(), 512 * 384);
    let frame = engine.frame(t1).expect("the latest frame");
    assert_eq!(frame.rgba.len(), 512 * 384 * 4);

    // 200%, held at the top left corner of the photo.
    let corner = View {
        zoom: 2.0,
        center_x: 0.0,
        center_y: 0.0,
        width: 300,
        height: 200,
    };
    let t2 = engine.view(opened.photo_id, corner).unwrap();
    let events = until_done(&rx, t2);
    assert!(
        matches!(
            events[0],
            Event::Frame {
                placement: Placement {
                    x_px: 0,
                    y_px: 0,
                    width_px: 300,
                    height_px: 200
                },
                ..
            }
        ),
        "{events:?}"
    );
    assert!(engine.frame(t1).is_none(), "only the latest frame is kept");
    let zoomed = engine.frame(t2).unwrap();
    // Each photo pixel is two screen pixels across.
    assert_eq!(zoomed.rgba[0..4], zoomed.rgba[4..8]);

    // Refused, with what was wrong, and nothing renders: no ticket is used up.
    let id = opened.photo_id;
    let refused =
        |patch_json: serde_json::Value| engine.apply(id, &patch(patch_json)).unwrap_err().kind();
    assert_eq!(
        refused(serde_json::json!({ "ag_exposure_wb": { "nope": 1 } })),
        ErrorKind::UnknownSetting
    );
    assert_eq!(
        refused(serde_json::json!({ "ag_nothing": { "x": 1 } })),
        ErrorKind::UnknownTool
    );
    assert_eq!(
        refused(serde_json::json!({ "rt_capture_sharpen": { "iterations": 2.5 } })),
        ErrorKind::BadValue
    );
    let unknown = engine
        .apply(PhotoId(99), &EditPatch::default())
        .unwrap_err();
    assert_eq!(unknown.kind(), ErrorKind::UnknownPhoto);
    let flat = View {
        zoom: 0.0,
        ..corner
    };
    assert_eq!(
        engine.view(id, flat).unwrap_err().kind(),
        ErrorKind::BadView
    );
    let t3 = engine.apply(id, &EditPatch::default()).unwrap().ticket;
    assert_eq!(t3, RenderTicket(t2.0 + 1));
    until_done(&rx, t3);
}

/// Settings for the test tools below.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Step {
    n: i32,
}

impl argentum_engine::Settings for Step {
    const VERSION: u32 = 1;
}

/// A display tool that takes 300 ms, and counts its runs.
macro_rules! slow_tool {
    ($name:ident, $id:literal, $runs:ident) => {
        static $runs: AtomicUsize = AtomicUsize::new(0);

        #[derive(Default)]
        struct $name;

        impl Tool for $name {
            const ID: &'static str = $id;
            const STAGE: Stage = Stage::SceneLinear;
            const SECTION: Section = Section::Effects;
            type Settings = Step;

            fn process(&self, _: &Step, _: &Image, _: &Context) -> Result<Option<Image>> {
                $runs.fetch_add(1, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(300));
                Ok(None)
            }
        }
    };
}

slow_tool!(AgSlowA, "ag_slow_a", RUNS_A);
slow_tool!(AgSlowB, "ag_slow_b", RUNS_B);

#[test]
fn renders_are_latest_wins() {
    let (engine, rx) = engine_with(|| standard().with::<AgSlowA>().with::<AgSlowB>());
    let id = engine.insert(common::source(256, 192)).photo_id;

    // t1 starts, and is in the first slow tool when t2 and t3 arrive: they
    // are asked for as soon as it has started there.
    let t1 = engine
        .apply(
            id,
            &patch(serde_json::json!({
                "rt_capture_sharpen": { "capture": false },
                "ag_slow_a": { "n": 1 },
            })),
        )
        .unwrap()
        .ticket;
    let waited = Instant::now();
    while RUNS_A.load(Ordering::SeqCst) == 0 {
        assert!(
            waited.elapsed() < Duration::from_secs(30),
            "t1 never reached ag_slow_a"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    let t2 = engine
        .apply(id, &patch(serde_json::json!({ "ag_slow_a": { "n": 2 } })))
        .unwrap()
        .ticket;
    let t3 = engine
        .apply(id, &patch(serde_json::json!({ "ag_slow_a": { "n": 3 } })))
        .unwrap()
        .ticket;

    let events = until_done(&rx, t3);
    let tickets: Vec<_> = events
        .iter()
        .map(|e| match e {
            Event::Frame { ticket, .. } | Event::Histogram { ticket, .. } => *ticket,
            Event::Error { .. } => panic!("{e:?}"),
        })
        .collect();
    assert_eq!(tickets, [t3, t3], "only the newest render reports");
    // t1 finished the step it was in, and gave up before the next one; t2
    // was replaced before it started.
    assert_eq!(RUNS_A.load(Ordering::SeqCst), 2, "t1 and t3");
    assert_eq!(RUNS_B.load(Ordering::SeqCst), 1, "t3 alone");
    assert!(engine.frame(t1).is_none() && engine.frame(t2).is_none());
    assert!(engine.frame(t3).is_some());
}

/// A tool that fails when asked to.
#[derive(Default)]
struct AgFails;

impl Tool for AgFails {
    const ID: &'static str = "ag_fails";
    const STAGE: Stage = Stage::SceneLinear;
    const SECTION: Section = Section::Effects;
    type Settings = Step;

    fn process(&self, s: &Step, _: &Image, _: &Context) -> Result<Option<Image>> {
        if s.n == 1 {
            return Err(Error::Tool {
                tool: Self::ID,
                message: "asked to fail".into(),
            });
        }
        Ok(None)
    }
}

#[test]
fn a_failed_render_says_so() {
    let (engine, rx) = engine_with(|| standard().with::<AgFails>());
    let id = engine.insert(common::source(128, 96)).photo_id;
    let t = engine
        .apply(
            id,
            &patch(serde_json::json!({
                "rt_capture_sharpen": { "capture": false },
                "ag_fails": { "n": 1 },
            })),
        )
        .unwrap()
        .ticket;
    let events = until_done(&rx, t);
    let [
        Event::Error {
            ticket,
            kind,
            message,
        },
    ] = &events[..]
    else {
        panic!("{events:?}")
    };
    assert_eq!((*ticket, *kind), (Some(t), ErrorKind::Render));
    assert!(message.contains("asked to fail"), "{message}");
}

/// `open` reports the developed size without developing: it has to agree
/// with what the develop then makes.
#[test]
fn the_size_open_reports_is_the_developed_size() {
    let mut cropped = common::scene(640, 480);
    cropped.active_area = Some(Rect::new(Point::new(12, 8), Dim2::new(616, 464)));
    cropped.crop_area = Some(Rect::new(Point::new(20, 14), Dim2::new(590, 440)));
    let mut sources = vec![
        ("synthetic".to_string(), common::source(320, 240)),
        ("synthetic, cropped".to_string(), Source::new(9, cropped)),
    ];
    for path in common::raws("the_size_open_reports_is_the_developed_size") {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        sources.push((name, common::open(&path)));
    }
    let mut p = Pipeline::standard().unwrap();
    p.set_json(
        "rt_capture_sharpen",
        1,
        serde_json::json!({ "capture": false }),
    )
    .unwrap();
    for (name, source) in sources {
        let image = p.render(&source, Size::Full).unwrap();
        let rgb = image.as_rgb().unwrap();
        eprintln!("  {name:<52} {:?}", source.size_px());
        assert_eq!(source.size_px(), (rgb.width, rgb.height), "{name}");
    }
}
