//! Which clipping view the photo is being shown in. Ours.
//!
//! RapidRAW has one clipping warning: any channel over the line goes red, any
//! pixel under it goes blue. That answers "is something clipped" and not the
//! question you ask next, which is *what* clipped — because a highlight where
//! only red has gone is recoverable and one where all three have gone is not.
//!
//! So the button cycles rather than toggles: off, all channels, then red, green
//! and blue on their own.
//!
//! WHY NOT ONE VIEW WITH THE CHANNELS COLOUR-CODED
//!
//! That was the first version, and it was better on paper: one look tells you
//! whether a highlight is savable, with no remembering. It was also misread
//! within seconds of being shown, because in the first mode red already means
//! *blown highlight* and in that one it would have meant *the red channel*.
//! Two meanings on one button. Red is blown and blue is crushed in every mode
//! here, and which channel you are looking at is the thing the button says.
//!
//! WHY THE FIELD IS STILL CALLED `showClipping`
//!
//! Because it is theirs, and it is stored per photo alongside every other
//! adjustment. Their button reads it as a truthy value and highlights when it is
//! set, which a mode number satisfies without them knowing anything about modes.
//! Adding a second field of our own would have meant a new key in the saved
//! adjustments — and every key there is part of the thumbnail cache hash, so a
//! new one rebuilds the user's whole library once for nothing.

/// Off. The photo as it is.
pub const OFF: u32 = 0;
/// Any channel: red for clipped, blue for crushed. RapidRAW's original.
pub const ALL: u32 = 1;
/// One channel at a time, same two colours.
pub const RED: u32 = 2;
pub const GREEN: u32 = 3;
pub const BLUE: u32 = 4;
/// How many steps the button cycles through, off included.
pub const COUNT: u32 = 5;

/// Read the mode out of the adjustments the frontend sent.
///
/// Accepts the old `true`/`false` as well as a number, because sidecars written
/// before this existed hold a boolean and must keep meaning what they meant.
pub fn mode(js_adjustments: &serde_json::Value) -> u32 {
    let value = &js_adjustments["showClipping"];
    if let Some(n) = value.as_u64() {
        return if n < COUNT as u64 { n as u32 } else { OFF };
    }
    if value.as_bool().unwrap_or(false) { ALL } else { OFF }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_missing_setting_is_off() {
        assert_eq!(mode(&json!({})), OFF);
        assert_eq!(mode(&json!({ "showClipping": null })), OFF);
    }

    /// Sidecars written before modes existed hold a boolean, and it has to keep
    /// meaning exactly what it meant.
    #[test]
    fn the_old_boolean_still_works() {
        assert_eq!(mode(&json!({ "showClipping": true })), ALL);
        assert_eq!(mode(&json!({ "showClipping": false })), OFF);
    }

    #[test]
    fn the_modes_come_through_as_numbers() {
        assert_eq!(mode(&json!({ "showClipping": 0 })), OFF);
        assert_eq!(mode(&json!({ "showClipping": 1 })), ALL);
        assert_eq!(mode(&json!({ "showClipping": 2 })), RED);
        assert_eq!(mode(&json!({ "showClipping": 3 })), GREEN);
        assert_eq!(mode(&json!({ "showClipping": 4 })), BLUE);
    }

    /// A number nobody wrote on purpose must land somewhere real rather than
    /// reaching past the end of the list.
    #[test]
    fn rubbish_lands_on_a_real_mode() {
        assert_eq!(mode(&json!({ "showClipping": 99 })), OFF);
        assert_eq!(mode(&json!({ "showClipping": "yes" })), OFF);
    }
}
