//! Which arrangement of the editor's panels shows, and how the parts only the
//! RAWmakase arrangement has were left. Ours.
//!
//! Classic is today's arrangement and stays the default until another is
//! chosen. RAWmakase arranges the same panels the way RAWmakase 0.3 does
//! (src/app/workspace.rs, side_tabs.rs): folders on the left, the photo with
//! its toolbar under it, the adjustments on the right with the Navigator and
//! Histogram over them, a tab panel beside them, and a slim rail at the edge.
//!
//! Only the interface reads either value; no photo renders differently. Both
//! live in argentum-processing.json through `ag_settings`, beside Compact
//! panels. Everything else the arrangement remembers (panel widths, which
//! panels are folded, which adjustment sections are open) is today's own
//! workspace state, saved where it always was.

use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::ag_settings;

const LAYOUT_KEY: &str = "editorLayout";
const PANELS_KEY: &str = "editorLayoutPanels";

/// The arrangement of the editor. Crosses to the interface as its lowercase
/// name.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum EditorLayout {
    /// Today's arrangement.
    #[default]
    Classic,
    /// RAWmakase 0.3's, built from the same panels.
    Rawmakase,
}

impl EditorLayout {
    /// The layout a name stands for. The name comes from a browser, so any
    /// string is possible; one this build does not know is Classic, which is
    /// what the editor showed before the choice existed.
    pub fn from_name(name: &str) -> Self {
        serde_json::from_value(Value::from(name)).unwrap_or_default()
    }
}

/// What the RAWmakase arrangement's own parts were left as. The interface
/// owns the tab ids, the widths and what they start as; this only keeps them.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LayoutPanels {
    /// The Navigator section is open.
    pub navigator_open: bool,
    /// The tab open beside the adjustments, or none while the tab panel is
    /// folded.
    pub tab: Option<String>,
    /// The tab panel's width, in CSS pixels.
    pub tab_width_px: u16,
}

/// The chosen arrangement, or Classic if none has been chosen or the file
/// cannot be read.
pub fn layout(library: &Path) -> EditorLayout {
    ag_settings::get(library, LAYOUT_KEY)
        .and_then(|value| value.as_str().map(EditorLayout::from_name))
        .unwrap_or_default()
}

/// Choose an arrangement.
pub fn save_layout(library: &Path, layout: EditorLayout) -> Result<(), String> {
    let value = serde_json::to_value(layout).map_err(|e| e.to_string())?;
    ag_settings::set(library, LAYOUT_KEY, value)
}

/// The arrangement's parts as last left, or `None` if never saved or not
/// readable as panels, for the interface to start them its own way.
pub fn panels(library: &Path) -> Option<LayoutPanels> {
    ag_settings::get(library, PANELS_KEY).and_then(|value| serde_json::from_value(value).ok())
}

/// Remember the arrangement's parts.
pub fn save_panels(library: &Path, panels: &LayoutPanels) -> Result<(), String> {
    let value = serde_json::to_value(panels).map_err(|e| e.to_string())?;
    ag_settings::set(library, PANELS_KEY, value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn scratch(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("argentum-editor-layout-{label}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch");
        dir
    }

    #[test]
    fn classic_until_another_is_chosen() {
        let dir = scratch("default");
        assert_eq!(layout(&dir), EditorLayout::Classic);
    }

    #[test]
    fn the_chosen_layout_is_what_is_read_back() {
        let dir = scratch("roundtrip");
        save_layout(&dir, EditorLayout::Rawmakase).expect("write");
        assert_eq!(layout(&dir), EditorLayout::Rawmakase);
        save_layout(&dir, EditorLayout::Classic).expect("write");
        assert_eq!(layout(&dir), EditorLayout::Classic);
    }

    #[test]
    fn names_cross_as_lowercase_and_unknown_ones_are_classic() {
        assert_eq!(
            EditorLayout::from_name("rawmakase"),
            EditorLayout::Rawmakase
        );
        assert_eq!(EditorLayout::from_name("classic"), EditorLayout::Classic);
        assert_eq!(EditorLayout::from_name("Rawmakase"), EditorLayout::Classic);
        assert_eq!(EditorLayout::from_name(""), EditorLayout::Classic);
        assert_eq!(
            serde_json::to_value(EditorLayout::Rawmakase).expect("serialise"),
            Value::from("rawmakase"),
        );
    }

    #[test]
    fn panels_are_none_until_saved() {
        let dir = scratch("panels-default");
        assert_eq!(panels(&dir), None);
    }

    #[test]
    fn saved_panels_are_what_is_read_back() {
        let dir = scratch("panels-roundtrip");
        let left = LayoutPanels {
            navigator_open: false,
            tab: None,
            tab_width_px: 300,
        };
        save_panels(&dir, &left).expect("write");
        assert_eq!(panels(&dir), Some(left));
    }

    #[test]
    fn a_value_that_is_not_panels_reads_as_none() {
        let dir = scratch("panels-foreign");
        ag_settings::set(
            &dir,
            PANELS_KEY,
            serde_json::json!({ "navigatorOpen": false }),
        )
        .expect("write");
        assert_eq!(panels(&dir), None);
        ag_settings::set(&dir, PANELS_KEY, Value::from("not an object")).expect("write");
        assert_eq!(panels(&dir), None);
    }

    /// The layout and its panels share a file with every other preference of
    /// ours; saving them must not disturb a neighbour.
    #[test]
    fn saving_leaves_the_other_preferences_alone() {
        let dir = scratch("neighbours");
        ag_settings::set(&dir, "compactSliders", Value::Bool(true)).expect("write");
        save_layout(&dir, EditorLayout::Rawmakase).expect("write");
        let left = LayoutPanels {
            navigator_open: true,
            tab: Some("history".to_string()),
            tab_width_px: 245,
        };
        save_panels(&dir, &left).expect("write");
        assert_eq!(
            ag_settings::get(&dir, "compactSliders"),
            Some(Value::Bool(true))
        );
    }
}
