//! Closing a photo and opening it again with the edit it had: what the app
//! does to keep only a few photos' decoded data in memory.

mod common;

use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use argentum_engine::contract::{EditDocument, EditPatch, Event, RenderTicket, View};
use argentum_engine::{Engine, Error, Pipeline};

fn patch(json: serde_json::Value) -> EditPatch {
    serde_json::from_value(json).unwrap()
}

fn engine() -> (Engine, Receiver<Event>) {
    let (tx, rx) = channel();
    let engine = Engine::new(move |e| {
        let _ = tx.send(e);
    })
    .unwrap();
    (engine, rx)
}

/// The pixels of `ticket`'s frame.
fn pixels(engine: &Engine, rx: &Receiver<Event>, ticket: RenderTicket) -> Vec<u8> {
    loop {
        match rx.recv_timeout(Duration::from_secs(60)).expect("an event") {
            Event::Frame { ticket: t, .. } if t == ticket => {
                return engine.frame(ticket).expect("the latest frame").rgba.clone();
            }
            Event::Error { message, .. } => panic!("{message}"),
            _ => {}
        }
    }
}

const VIEW: View = View {
    zoom: 0.5,
    center_x: 0.5,
    center_y: 0.5,
    width: 128,
    height: 96,
};

#[test]
fn edit_is_the_photos_edit_with_every_accepted_patch() {
    let (engine, _rx) = engine();
    let opened = engine.insert(common::source(256, 192));
    assert_eq!(engine.edit(opened.photo_id).unwrap(), opened.edit);

    engine
        .apply(
            opened.photo_id,
            &patch(serde_json::json!({ "ag_exposure_wb": { "exposure": 0.5 } })),
        )
        .unwrap();
    assert!(
        engine
            .apply(
                opened.photo_id,
                &patch(serde_json::json!({ "ag_exposure_wb": { "nope": 1 } }))
            )
            .is_err()
    );
    let edit = engine.edit(opened.photo_id).unwrap();
    assert_eq!(edit.tools["ag_exposure_wb"].settings["exposure"], 0.5);
}

#[test]
fn a_closed_photo_is_gone_and_hands_back_its_edit() {
    let (engine, _rx) = engine();
    let opened = engine.insert(common::source(256, 192));
    engine
        .apply(
            opened.photo_id,
            &patch(serde_json::json!({ "ag_exposure_wb": { "exposure": 1.0 } })),
        )
        .unwrap();
    let kept = engine.close(opened.photo_id).unwrap();
    assert_eq!(kept.tools["ag_exposure_wb"].settings["exposure"], 1.0);

    assert!(matches!(
        engine.edit(opened.photo_id),
        Err(Error::UnknownPhoto(_))
    ));
    assert!(matches!(
        engine.view(opened.photo_id, VIEW),
        Err(Error::UnknownPhoto(_))
    ));
    assert!(
        matches!(engine.close(opened.photo_id), Err(Error::UnknownPhoto(_))),
        "closed once only"
    );
}

#[test]
fn reopened_with_its_edit_it_renders_the_same() {
    let (engine, rx) = engine();
    let first = engine.insert(common::source(256, 192));
    let changes = patch(serde_json::json!({
        "ag_exposure_wb": { "exposure": 0.7, "red": 1.2 },
        "rt_capture_sharpen": { "capture": false }
    }));
    engine.apply(first.photo_id, &changes).unwrap();
    let before = pixels(&engine, &rx, engine.view(first.photo_id, VIEW).unwrap());

    let kept = engine.close(first.photo_id).unwrap();
    let again = engine.insert_with(common::source(256, 192), &kept).unwrap();
    assert_ne!(again.photo_id, first.photo_id, "a new id");
    assert_eq!(again.edit, kept);
    let after = pixels(&engine, &rx, engine.view(again.photo_id, VIEW).unwrap());
    assert_eq!(before, after);
}

#[test]
fn an_edit_that_cannot_be_read_opens_nothing() {
    let (engine, _rx) = engine();
    let mut edit = Pipeline::standard().unwrap().edit();
    edit.tools.get_mut("ag_exposure_wb").unwrap().v = 99;
    assert!(matches!(
        engine.insert_with(common::source(64, 48), &edit),
        Err(Error::BadEdit(_))
    ));
    let unknown = EditDocument {
        version: 2,
        ..EditDocument::new()
    };
    assert!(
        engine
            .insert_with(common::source(64, 48), &unknown)
            .is_err()
    );
    // Nothing was opened: the next photo is the first.
    assert_eq!(engine.insert(common::source(64, 48)).photo_id.0, 1);
}
