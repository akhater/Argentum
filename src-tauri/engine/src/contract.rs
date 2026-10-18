//! The contract between the engine and the editor's interface.
//!
//! Every type the interface sees is here and nowhere else.
//! `src/shell/contract.generated.ts` is generated from this file, and
//! `src/shell/contract.fixtures.json` holds an example of each - both by
//! `tests/contract.rs`, which fails when either is stale. Regenerate with
//! `AG_WRITE_CONTRACT=1 cargo test --test contract`.
//!
//! A setting's value is JSON: a number, a boolean, or a string for a choice.
//! The TypeScript side calls it `SettingValue`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The version of `EditDocument`'s own shape. Each tool's settings carry
/// their own version, in `ToolEdit::v`.
pub const EDIT_VERSION: u32 = 1;

/// How many bins a histogram event has: one per 8-bit level.
pub const HISTOGRAM_BINS: usize = 256;

// How events and frames cross to the interface. The engine writes the frame
// block (`Frame::block`); the bridge and the interface's client read these
// rather than writing them again.

/// The name every engine event is sent under, carrying the `Event` as this
/// file writes it.
pub const ENGINE_EVENT: &str = "ag-engine";

/// A frame block's header: the ticket, the width and the height in pixels,
/// little-endian u32s. The RGBA rows follow.
pub const FRAME_HEADER_BYTES: usize = 12;

/// What fetching a frame answers when the block went to the page as a shared
/// buffer instead of as the answer.
pub const FRAME_SHARED: &str = "shared";

/// The key, in a shared buffer's additional data, whose value is the frame's
/// ticket.
pub const FRAME_SHARED_KEY: &str = "agFrame";

/// Whose code a tool is: the first two letters of its id.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum Origin {
    #[serde(rename = "dt")]
    Darktable,
    #[serde(rename = "rt")]
    RawTherapee,
    #[serde(rename = "rr")]
    RapidRaw,
    #[serde(rename = "rm")]
    Rawmakase,
    #[serde(rename = "ag")]
    Argentum,
}

impl Origin {
    /// Every origin. With `code`, the one place the prefixes are written;
    /// a test holds the wire names above to them.
    pub const ALL: [Origin; 5] = [
        Origin::Darktable,
        Origin::RawTherapee,
        Origin::RapidRaw,
        Origin::Rawmakase,
        Origin::Argentum,
    ];

    /// Its two letters: on the wire, and before the `_` of a tool's id.
    pub const fn code(self) -> &'static str {
        match self {
            Origin::Darktable => "dt",
            Origin::RawTherapee => "rt",
            Origin::RapidRaw => "rr",
            Origin::Rawmakase => "rm",
            Origin::Argentum => "ag",
        }
    }

    /// The origin an id names - its code, `_`, a name - or `None` when it
    /// names none or is not a usable id: lowercase ASCII letters, digits and
    /// `_`. Ids are keys in saved edits, so they stay that plain. `const` so
    /// that `Registry::with` can refuse a bad id at compile time.
    pub const fn of(id: &str) -> Option<Origin> {
        let b = id.as_bytes();
        let mut i = 0;
        while i < b.len() {
            let c = b[i];
            if !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_') {
                return None;
            }
            i += 1;
        }
        let mut k = 0;
        while k < Self::ALL.len() {
            let code = Self::ALL[k].code().as_bytes();
            let mut same = b.len() > code.len() + 1 && b[code.len()] == b'_';
            let mut j = 0;
            while same && j < code.len() {
                same = b[j] == code[j];
                j += 1;
            }
            if same {
                return Some(Self::ALL[k]);
            }
            k += 1;
        }
        None
    }
}
/// Where in the pipeline a tool runs: on the sensor data before demosaic, or
/// on linear light after it. A stage for the picture shaped for a screen
/// comes with the first tool that needs one.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum Stage {
    #[serde(rename = "raw")]
    Raw,
    #[serde(rename = "scene")]
    SceneLinear,
}

/// The panel a tool is shown in. More can be added; `SECTIONS` says their
/// order.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum Section {
    Basic,
    Tone,
    Color,
    Detail,
    Lens,
    Effects,
}

/// Every section, in the order the interface shows them.
pub const SECTIONS: [Section; 6] = [
    Section::Basic,
    Section::Tone,
    Section::Color,
    Section::Detail,
    Section::Lens,
    Section::Effects,
];

/// One section, as the interface shows it.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SectionDescriptor {
    pub id: Section,
    /// An i18n key: `sections.<id>`.
    pub label: String,
}

impl Section {
    /// Its id on the wire.
    pub const fn id(self) -> &'static str {
        match self {
            Section::Basic => "basic",
            Section::Tone => "tone",
            Section::Color => "color",
            Section::Detail => "detail",
            Section::Lens => "lens",
            Section::Effects => "effects",
        }
    }
}

/// Every section, in order.
pub fn sections() -> Vec<SectionDescriptor> {
    SECTIONS
        .iter()
        .map(|&id| SectionDescriptor {
            id,
            label: format!("sections.{}", id.id()),
        })
        .collect()
}

/// How a float setting's control moves: evenly, or by ratio - so that, on a
/// multiplier from 0.05 to 20, 1 sits in the middle.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum SettingScale {
    Linear,
    Log,
}

/// A setting that only means something while another setting has one value:
/// a manual radius, say, only while the automatic one is off.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct EnabledWhen {
    /// The other setting's key, in the same tool.
    pub key: String,
    #[cfg_attr(feature = "ts", ts(type = "SettingValue"))]
    pub equals: Value,
}

/// What kind of control a setting is.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum SettingKind {
    Float,
    Int,
    Bool,
    Choice,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Choice {
    pub value: String,
    /// An i18n key.
    pub label: String,
}

/// One setting of a tool, as a panel shows it.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SettingSpec {
    /// Its name in the tool's settings.
    pub key: String,
    /// An i18n key: `tools.<tool id>.<key>`.
    pub label: String,
    pub kind: SettingKind,
    #[cfg_attr(feature = "ts", ts(type = "SettingValue"))]
    pub default: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub max: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub step: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub unit: Option<String>,
    /// Float settings only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub scale: Option<SettingScale>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub choices: Option<Vec<Choice>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub enabled_when: Option<EnabledWhen>,
    /// Shown under "More" rather than in the panel's first view.
    pub advanced: bool,
}

/// One tool, as the interface builds its panel.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ToolDescriptor {
    /// Stable for ever: origin prefix and a name, e.g. `rt_capture_sharpen`.
    pub id: String,
    pub origin: Origin,
    /// An i18n key: `tools.<tool id>.label`.
    pub label: String,
    pub section: Section,
    /// The tool's place in the pipeline, and in its section.
    pub order: u32,
    pub stage: Stage,
    /// The version of its settings.
    pub version: u32,
    pub settings: Vec<SettingSpec>,
}

/// One tool's settings in an edit.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ToolEdit {
    /// The version of the tool's settings these were written at.
    pub v: u32,
    #[cfg_attr(feature = "ts", ts(type = "Record<string, SettingValue>"))]
    pub settings: BTreeMap<String, Value>,
}

/// Every tool's settings for one photo.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct EditDocument {
    /// `EDIT_VERSION`.
    pub version: u32,
    #[cfg_attr(feature = "ts", ts(type = "Record<string, ToolEdit>"))]
    pub tools: BTreeMap<String, ToolEdit>,
}

/// A change to an edit: `{ toolId: { key: value } }`, merged into it. `null`
/// puts a setting back to its default. Only the tools it names, and the
/// tools after them, run again.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct EditPatch(
    #[cfg_attr(
        feature = "ts",
        ts(type = "Record<string, Record<string, SettingValue | null>>")
    )]
    pub BTreeMap<String, BTreeMap<String, Value>>,
);

/// Names an open photo.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct PhotoId(pub u32);

/// Names one requested render. A newer ticket cancels every older one.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RenderTicket(pub u32);

/// What `apply` returns: the ticket of the render it started, and the edit
/// as the engine took it - clamped, with the defaults filled in. What the
/// interface shows should be this, not what it sent.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Applied {
    pub ticket: RenderTicket,
    pub edit: EditDocument,
}

/// What `open` returns.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct Opened {
    pub photo_id: PhotoId,
    /// The developed picture's size, in photo pixels.
    pub width: u32,
    pub height: u32,
    pub edit: EditDocument,
}

/// The part of the photo on screen.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct View {
    /// Screen pixels per photo pixel: 1 is 100%.
    pub zoom: f32,
    /// The point of the photo at the centre of the view, 0..1 across and
    /// down.
    pub center_x: f32,
    pub center_y: f32,
    /// The view's size in device pixels: CSS pixels times the device pixel
    /// ratio, so a frame fills it pixel for pixel.
    pub width: u32,
    pub height: u32,
}

/// What went wrong, as a kind the interface can act on.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum ErrorKind {
    /// No open photo has that id.
    UnknownPhoto,
    /// A view with no size, a zoom that is not positive, or a centre that is
    /// not a number.
    BadView,
    /// A patch or an edit names a tool this engine does not have.
    UnknownTool,
    /// A patch names a setting the tool does not have.
    UnknownSetting,
    /// A value of the wrong type, or not one of its choices.
    BadValue,
    /// An edit this engine cannot read: another shape version, or a tool's
    /// settings saved by a newer engine.
    BadEdit,
    /// The file could not be read, or is not a RAW the engine can decode.
    Decode,
    /// A tool needs the GPU and there is none.
    NoGpu,
    /// A tool failed while it rendered.
    Render,
    /// A fault of the engine's own, not of what it was asked.
    Internal,
}

/// A refused call: what went wrong, and in words.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct EngineError {
    pub kind: ErrorKind,
    pub message: String,
}

/// Where a frame sits in its view, in device pixels: its top left corner,
/// and its size. On an axis where the photo is smaller than the view, the
/// frame is all of it, centred; where it is larger, the frame fills the view.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct Placement {
    pub x_px: u32,
    pub y_px: u32,
    pub width_px: u32,
    pub height_px: u32,
}

/// What the engine tells the interface, as it happens.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "event", rename_all = "camelCase")]
pub enum Event {
    /// A render is ready: the view's part of the photo, in sRGB pixels, and
    /// where in the view it goes.
    Frame {
        ticket: RenderTicket,
        /// The photo it is a view of: a late frame of the photo before can
        /// be told apart and dropped.
        #[serde(rename = "photoId")]
        photo_id: PhotoId,
        placement: Placement,
    },
    /// `HISTOGRAM_BINS` bins of luminance, over the frame with the same
    /// ticket.
    Histogram {
        ticket: RenderTicket,
        #[serde(rename = "photoId")]
        photo_id: PhotoId,
        bins: Vec<u32>,
    },
    /// A render failed. Its ticket, when it was one.
    Error {
        #[cfg_attr(feature = "ts", ts(optional))]
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ticket: Option<RenderTicket>,
        kind: ErrorKind,
        message: String,
    },
}

impl EditDocument {
    pub fn new() -> Self {
        Self {
            version: EDIT_VERSION,
            tools: BTreeMap::new(),
        }
    }
}

impl Default for EditDocument {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins_come_from_one_table() {
        for origin in Origin::ALL {
            // The wire name is the code.
            assert_eq!(serde_json::to_value(origin).unwrap(), origin.code());
            let id = format!("{}_tool", origin.code());
            assert_eq!(Origin::of(&id), Some(origin));
            assert_eq!(Origin::of(&format!("{}_", origin.code())), None, "no name");
        }
        assert_eq!(Origin::of("xx_tool"), None);
        assert_eq!(Origin::of("dt_Highlights"), None, "lowercase only");
        assert_eq!(Origin::of("dt_high-lights"), None);
        assert_eq!(Origin::of("dthighlights"), None);
    }
}
