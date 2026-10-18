//! North star 5: budgets, measured. Each is a number set from its first
//! measurement and never to be raised.
//!
//! A test binary of its own, with one test in it: anything else running in
//! the same process shares the thread pool and would be timed with it.

mod common;

use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use argentum_engine::Engine;
use argentum_engine::contract::{EditPatch, Event, PhotoId, RenderTicket, View};

/// A slider tick at preview size, end to end through the engine: the patch
/// in, the frame event out. That is the render (the tool that changed and
/// every tool after it), the view cut out and encoded, and the event.
///
/// Set from its first measurement: 34 MP Canon CR3, Intel Arc 140T,
/// 2026-10-18, a 1600 x 1067 view of the whole photo - median 15 to 18 ms
/// over three runs, with room for a busy machine. Checked with every tool
/// on and the exposure slider moving: the worst case, since every tool after
/// exposure runs again.
///
/// It does not hold yet. With the basic tools on the CPU the same tick took
/// a median of 392 ms (2026-10-18, other builds running), most of it the
/// tone sliders: see "Not here yet" in the README. The budget stays.
const SLIDER_TICK: Duration = Duration::from_millis(25);

fn wait(rx: &Receiver<Event>, ticket: RenderTicket) {
    loop {
        match rx.recv_timeout(Duration::from_secs(60)).expect("an event") {
            Event::Frame { ticket: t, .. } if t == ticket => return,
            Event::Error { message, .. } => panic!("{message}"),
            _ => {}
        }
    }
}

/// Ten ticks of one slider, each waited for: their median, and all of them.
fn ticks(
    engine: &Engine,
    rx: &Receiver<Event>,
    photo: PhotoId,
    label: &str,
    tick: impl Fn(u32) -> serde_json::Value,
) -> Duration {
    let mut times: Vec<Duration> = (1..=10)
        .map(|i| {
            let patch: EditPatch = serde_json::from_value(tick(i)).unwrap();
            let started = Instant::now();
            wait(rx, engine.apply(photo, &patch).unwrap().ticket);
            started.elapsed()
        })
        .collect();
    eprintln!(
        "  {label:<44} {} ms",
        times
            .iter()
            .map(|t| format!("{:.1}", ms(t)))
            .collect::<Vec<_>>()
            .join(", ")
    );
    times.sort();
    // The median, so one tick that waited on the system does not decide it.
    times[times.len() / 2]
}

fn ms(d: &Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

#[test]
fn a_slider_tick_at_preview_size() {
    let Some(path) = common::raw("a_slider_tick_at_preview_size") else {
        return;
    };
    let (tx, rx) = channel();
    let engine = Engine::new(move |e| {
        let _ = tx.send(e);
    })
    .unwrap();

    let opened = engine.open(&path).unwrap();
    let photo = opened.photo_id;
    let (w, h) = (opened.width as f32, opened.height as f32);
    let view = View {
        zoom: (1600.0 / w).min(1067.0 / h),
        center_x: 0.5,
        center_y: 0.5,
        width: 1600,
        height: 1067,
    };
    let started = Instant::now();
    wait(&rx, engine.view(photo, view).unwrap());
    eprintln!(
        "\n  {}\n  first preview: {:.0} ms",
        path.display(),
        ms(&started.elapsed())
    );

    // As the photo opens: its own white balance and offset, DNG Standard on,
    // every other slider at zero.
    let exposure = |i: u32| serde_json::json!({ "ag_exposure": { "exposure_ev": i as f32 * 0.1 } });
    let opened_median = ticks(&engine, &rx, photo, "exposure, as opened:", exposure);

    // Every tool on.
    let on: EditPatch = serde_json::from_value(serde_json::json!({
        "rr_white_balance": { "temperature_k": 4300.0, "tint": 8.0 },
        "rr_basic_tone": {
            "contrast": 20.0, "highlights": -40.0, "shadows": 35.0, "whites": 15.0, "blacks": -10.0
        },
        "rr_presence": { "vibrance": 25.0, "saturation": 10.0 },
    }))
    .unwrap();
    wait(&rx, engine.apply(photo, &on).unwrap().ticket);
    let all_on = ticks(&engine, &rx, photo, "exposure, every tool on:", exposure);
    let shadows = ticks(
        &engine,
        &rx,
        photo,
        "shadows, every tool on:",
        |i| serde_json::json!({ "rr_basic_tone": { "shadows": 30.0 + i as f32 } }),
    );
    let vibrance = ticks(
        &engine,
        &rx,
        photo,
        "vibrance, every tool on:",
        |i| serde_json::json!({ "rr_presence": { "vibrance": 20.0 + i as f32 } }),
    );
    eprintln!(
        "  medians: as opened {:.1} ms, every tool on {:.1} ms (shadows {:.1}, vibrance {:.1})",
        ms(&opened_median),
        ms(&all_on),
        ms(&shadows),
        ms(&vibrance)
    );
    assert!(
        all_on <= SLIDER_TICK,
        "a slider tick with every tool on takes {:.1} ms, the budget is {} ms",
        ms(&all_on),
        SLIDER_TICK.as_millis()
    );
}
