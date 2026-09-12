//! Regression tests for upstream's cache keys. Ours.
//!
//! WHY THIS FILE EXISTS RATHER THAN A `#[cfg(test)]` BLOCK IN `cache_utils.rs`
//!
//! The functions under test are upstream's, in an upstream file, which has a
//! budget of eight added lines. A test module there would spend most of it on
//! something that is not a feature. The functions are `pub`, so the tests can
//! live here instead and cost that budget nothing.
//!
//! THE BUG THESE GUARD
//!
//! `calculate_transform_hash` identified an AI patch by the *length* of its
//! base64 rather than its contents:
//!
//! ```text
//! let data_len = patch.get("patchDataBase64")...len();
//! data_len.hash(&mut hasher);
//! ```
//!
//! Two different patches of the same base64 length therefore hashed the same,
//! and the cache served whichever arrived first — the wrong picture, with no
//! error anywhere. Base64 of a fixed-size region is very often the same length,
//! so this is not a remote possibility.
//!
//! Reported upstream as part of RapidRAW PR #1307; fixed here on 2026-09-13
//! rather than waiting, because it produces visibly wrong output.

#[cfg(test)]
mod tests {
    use crate::cache_utils::calculate_transform_hash;
    use serde_json::json;

    /// Two patches, same base64 length, different contents.
    #[test]
    fn patches_of_equal_length_do_not_share_a_hash() {
        let a = json!({ "aiPatches": [{ "id": "p1", "visible": true, "patchDataBase64": "AAAABBBB" }] });
        let b = json!({ "aiPatches": [{ "id": "p1", "visible": true, "patchDataBase64": "CCCCDDDD" }] });

        assert_eq!(
            a["aiPatches"][0]["patchDataBase64"].as_str().unwrap().len(),
            b["aiPatches"][0]["patchDataBase64"].as_str().unwrap().len(),
            "the test is meaningless unless the two are the same length",
        );
        assert_ne!(
            calculate_transform_hash(&a),
            calculate_transform_hash(&b),
            "two different patches of equal length must not share a cache key",
        );
    }

    /// The same, for the colour and mask halves of an unpacked patch.
    #[test]
    fn patch_colour_and_mask_are_hashed_by_content() {
        let base = |color: &str, mask: &str| {
            json!({ "aiPatches": [{
                "id": "p1",
                "visible": true,
                "patchData": { "color": color, "mask": mask },
            }] })
        };

        assert_ne!(
            calculate_transform_hash(&base("AAAA", "MMMM")),
            calculate_transform_hash(&base("BBBB", "MMMM")),
            "a different colour of the same length must change the key",
        );
        assert_ne!(
            calculate_transform_hash(&base("AAAA", "MMMM")),
            calculate_transform_hash(&base("AAAA", "NNNN")),
            "a different mask of the same length must change the key",
        );
    }

    /// And the obvious half: identical input still hashes identically, or the
    /// cache would never hit at all.
    #[test]
    fn identical_patches_still_share_a_hash() {
        let p = json!({ "aiPatches": [{ "id": "p1", "visible": true, "patchDataBase64": "AAAABBBB" }] });
        assert_eq!(calculate_transform_hash(&p), calculate_transform_hash(&p.clone()));
    }
}
