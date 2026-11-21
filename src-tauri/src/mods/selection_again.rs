//! AI masks found again on each photo a preset or a paste reaches. Ours, on
//! RapidRAW 1.6.7's idea.
//!
//! A preset or a paste carries a mask's outline as it was found on the photo it
//! was made on, so a Sky mask darkened the first photo's sky shape everywhere.
//! RapidRAW 1.6.7 strips an AI mask's outline when a preset goes on and makes it
//! again for the photo (07ef9211, a693d967: maskUtils.ts `stripAiMaskResults`,
//! PresetsPanel.tsx `resolvePresetAiMasks`, ai_jobs.rs). Ours does the same
//! through Argentum's own selections (selection.rs, text_mask.rs, face_mask.rs),
//! not their job queue and their models: the interface strips the outlines and
//! asks here for each photo's own (argentum/selection/foundAgain.ts).
//!
//! The picture is the open photo's when it is open (lib.rs
//! `get_cached_full_warped_image`), else the photo read from its file and
//! sidecar (warped_photo.rs). An Object's clicks were made on another photo:
//! they go to the same place in this one, as RapidRAW moves its Subject box to
//! a photo of another size (ai_jobs.rs `scaled_subject_box` @ a693d967).

use std::path::Path;
use std::sync::Arc;

use image::{DynamicImage, GenericImageView};
use serde::{Deserialize, Serialize};

use super::selection::{Bounds, Click, Frame, Kind, Selection};

/// The photo an Object's clicks were made on: its picture's size, from the
/// outline found there, and how it was turned.
#[derive(Debug, Clone, Copy, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct MadeOn {
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub orientation_steps: u8,
}

/// Which selection to make, with what it was made from.
#[derive(Debug, Clone, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Wanted {
    Subject,
    Sky,
    Background,
    Object {
        clicks: Vec<Click>,
        bounds: Option<Bounds>,
        from: MadeOn,
    },
    Text {
        words: String,
    },
    Face {
        parts: Vec<String>,
    },
}

/// One AI sub-mask to find again, by its id.
#[derive(Debug, Clone, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct Ask {
    pub id: String,
    pub wanted: Wanted,
}

/// What one sub-mask found on this photo: none when the selection failed, so
/// that mask stays empty and the rest still applies.
#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct Found {
    pub id: String,
    pub selection: Option<Selection>,
}

/// Where each set of models is: Subject, Sky, Background and Object's, and the
/// folder Text's and Face's are in.
pub struct ModelDirs<'a> {
    pub selection: &'a Path,
    pub masks: &'a Path,
}

/// The frame a photo's edit shows it in, as the interface sends it with each
/// selection (selection.tsx).
pub fn frame_of(adjustments: &serde_json::Value) -> Frame {
    Frame {
        rotation: adjustments["rotation"].as_f64().unwrap_or(0.0) as f32,
        flip_horizontal: adjustments["flipHorizontal"].as_bool().unwrap_or(false),
        flip_vertical: adjustments["flipVertical"].as_bool().unwrap_or(false),
        orientation_steps: adjustments["orientationSteps"].as_u64().unwrap_or(0) as u8,
    }
}

/// Every asked selection on `picture`, in order, each on its own: one that
/// fails is logged and found as none.
pub fn find_all(
    dirs: &ModelDirs,
    picture: &Arc<DynamicImage>,
    key: u64,
    frame: Frame,
    asks: &[Ask],
) -> Vec<Found> {
    asks.iter()
        .map(|ask| Found {
            id: ask.id.clone(),
            selection: find(dirs, picture, key, frame, &ask.wanted)
                .map_err(|e| log::warn!("AI mask {} not found again: {e}", ask.id))
                .ok(),
        })
        .collect()
}

fn find(
    dirs: &ModelDirs,
    picture: &Arc<DynamicImage>,
    key: u64,
    frame: Frame,
    wanted: &Wanted,
) -> Result<Selection, String> {
    let select = |kind| super::selection::select(dirs.selection, picture, key, kind, frame);
    match wanted {
        Wanted::Subject => select(Kind::Subject),
        Wanted::Sky => select(Kind::Sky),
        Wanted::Background => select(Kind::Background),
        Wanted::Object {
            clicks,
            bounds,
            from,
        } => {
            let (clicks, bounds) = moved(clicks, *bounds, *from, picture.dimensions(), frame);
            super::selection::select_object(dirs.selection, picture, key, &clicks, bounds, frame)
        }
        Wanted::Text { words } => super::text_mask::select(dirs.masks, picture, key, words, frame),
        Wanted::Face { parts } => super::face_mask::select(dirs.masks, picture, key, parts, frame),
    }
}

/// The picture's size as it is shown: a quarter turn swaps it.
fn turned(width: u32, height: u32, orientation_steps: u8) -> (f64, f64) {
    if orientation_steps % 2 == 1 {
        (height as f64, width as f64)
    } else {
        (width as f64, height as f64)
    }
}

/// Clicks and a box made on `from`, at the same place in the photo as it is
/// shown, on a picture `to` that is shown in `frame`. Clicks are in the shown
/// photo's pixels (selection.rs `Click`), so they scale with its size.
fn moved(
    clicks: &[Click],
    bounds: Option<Bounds>,
    from: MadeOn,
    to: (u32, u32),
    frame: Frame,
) -> (Vec<Click>, Option<Bounds>) {
    let (from_w, from_h) = turned(
        from.width.max(1),
        from.height.max(1),
        from.orientation_steps,
    );
    let (to_w, to_h) = turned(to.0, to.1, frame.orientation_steps);
    let (sx, sy) = (to_w / from_w, to_h / from_h);
    let clicks = clicks
        .iter()
        .map(|c| Click {
            x: c.x * sx,
            y: c.y * sy,
            positive: c.positive,
        })
        .collect();
    let bounds = bounds.map(|b| Bounds {
        start_x: b.start_x * sx,
        start_y: b.start_y * sy,
        end_x: b.end_x * sx,
        end_y: b.end_y * sy,
    });
    (clicks, bounds)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHOWN: Frame = Frame {
        rotation: 0.0,
        flip_horizontal: false,
        flip_vertical: false,
        orientation_steps: 0,
    };

    #[test]
    fn clicks_on_a_photo_of_the_same_size_stay_where_they_were() {
        let from = MadeOn {
            width: 600,
            height: 400,
            orientation_steps: 0,
        };
        let click = Click {
            x: 150.0,
            y: 100.0,
            positive: true,
        };
        let (clicks, bounds) = moved(&[click], None, from, (600, 400), SHOWN);
        assert_eq!(
            (clicks[0].x, clicks[0].y, clicks[0].positive),
            (150.0, 100.0, true)
        );
        assert!(bounds.is_none());
    }

    #[test]
    fn clicks_and_a_box_go_to_the_same_place_on_a_bigger_photo() {
        let from = MadeOn {
            width: 600,
            height: 400,
            orientation_steps: 0,
        };
        let click = Click {
            x: 150.0,
            y: 100.0,
            positive: false,
        };
        let bounds = Bounds {
            start_x: 60.0,
            start_y: 40.0,
            end_x: 300.0,
            end_y: 200.0,
        };
        let (clicks, bounds) = moved(&[click], Some(bounds), from, (1200, 800), SHOWN);
        assert_eq!(
            (clicks[0].x, clicks[0].y, clicks[0].positive),
            (300.0, 200.0, false)
        );
        let b = bounds.unwrap();
        assert_eq!(
            (b.start_x, b.start_y, b.end_x, b.end_y),
            (120.0, 80.0, 600.0, 400.0)
        );
    }

    #[test]
    fn a_turned_photo_is_measured_as_it_is_shown() {
        // Made on a landscape photo shown as it is; this one is a landscape
        // picture shown turned, so portrait: the click keeps its shown place.
        let from = MadeOn {
            width: 600,
            height: 400,
            orientation_steps: 0,
        };
        let turned_frame = Frame {
            orientation_steps: 1,
            ..SHOWN
        };
        let click = Click {
            x: 300.0,
            y: 200.0,
            positive: true,
        };
        let (clicks, _) = moved(&[click], None, from, (600, 400), turned_frame);
        assert_eq!((clicks[0].x, clicks[0].y), (200.0, 300.0));
    }

    #[test]
    fn the_frame_is_read_from_the_edit() {
        let edit = serde_json::json!({
            "rotation": 2.5, "flipHorizontal": true, "orientationSteps": 3, "exposure": 1.0
        });
        let frame = frame_of(&edit);
        assert_eq!(frame.rotation, 2.5);
        assert!(frame.flip_horizontal && !frame.flip_vertical);
        assert_eq!(frame.orientation_steps, 3);
    }

    #[test]
    fn a_selection_that_cannot_run_leaves_only_its_own_mask_empty() {
        // No models in these folders: every selection fails, none panics, and
        // each id comes back in order with nothing found.
        let empty = std::env::temp_dir().join(format!("ag-selection-again-{}", std::process::id()));
        std::fs::create_dir_all(&empty).unwrap();
        let dirs = ModelDirs {
            selection: &empty,
            masks: &empty,
        };
        let picture = Arc::new(DynamicImage::new_rgb8(64, 48));
        let asks = vec![
            Ask {
                id: "sky".into(),
                wanted: Wanted::Sky,
            },
            Ask {
                id: "words".into(),
                wanted: Wanted::Text {
                    words: "car".into(),
                },
            },
            Ask {
                id: "face".into(),
                wanted: Wanted::Face {
                    parts: vec!["faceSkin".into()],
                },
            },
        ];
        let found = find_all(&dirs, &picture, 1, SHOWN, &asks);
        let ids: Vec<&str> = found.iter().map(|f| f.id.as_str()).collect();
        assert_eq!(ids, ["sky", "words", "face"]);
        assert!(found.iter().all(|f| f.selection.is_none()));
        let _ = std::fs::remove_dir_all(&empty);
    }

    #[test]
    fn asks_read_as_the_interface_sends_them() {
        let asks: Vec<Ask> = serde_json::from_value(serde_json::json!([
            { "id": "a", "wanted": { "kind": "subject" } },
            { "id": "b", "wanted": { "kind": "object", "clicks": [{ "x": 1.0, "y": 2.0, "positive": true }],
                                     "bounds": null, "from": { "width": 10, "height": 20 } } },
            { "id": "c", "wanted": { "kind": "text", "words": "car" } },
            { "id": "d", "wanted": { "kind": "face", "parts": ["lips"] } }
        ]))
        .unwrap();
        assert!(matches!(asks[0].wanted, Wanted::Subject));
        assert!(matches!(&asks[1].wanted, Wanted::Object { from, .. } if from.height == 20));
        assert!(matches!(&asks[2].wanted, Wanted::Text { words } if words == "car"));
        assert!(matches!(&asks[3].wanted, Wanted::Face { parts } if parts == &["lips"]));
    }
}
