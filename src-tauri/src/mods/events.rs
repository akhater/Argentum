//! The names of the events the app sends to the interface, defined once.
//!
//! Each is emitted from Rust with `app.emit(events::NAME, payload)` and
//! heard in the interface as `AG_EVENTS.NAME`, which `mods/ag_contract.rs`
//! writes into `src/argentum/ag.generated.ts` from this list. A name typed
//! on one side only (a renamed event, a listener nothing emits) is a feature
//! that stops with no compile error; the tests below and `npm run
//! check:wiring` fail on it instead. Ours.
//!
//! Left out on purpose: the look provider's own event (`mods/lut_provider.rs`)
//! and the engine's (`ENGINE_EVENT`, from its contract).
//!
//! Older files of the app still pass their names as literals
//! (`app.emit("culling-start", ..)`). They are in the list too: the tests
//! fail when such a literal is not listed, or a listed name is emitted
//! nowhere.

macro_rules! events {
    ($($id:ident = $name:literal;)*) => {
        // Older files emit by literal, so not every constant is used yet.
        $(#[allow(dead_code)] pub const $id: &str = $name;)*
        /// Every event: the constant's name, then the event's.
        #[cfg(test)]
        pub const ALL: &[(&str, &str)] = &[$((stringify!($id), $name)),*];
    };
}

events! {
    AG_MODEL_DOWNLOAD = "ag-model-download";
    AI_CONNECTOR_STATUS_UPDATE = "ai-connector-status-update";
    AI_MODEL_DOWNLOAD_FINISH = "ai-model-download-finish";
    AI_MODEL_DOWNLOAD_START = "ai-model-download-start";
    ANALYTICS_UPDATE = "analytics-update";
    BATCH_EXPORT_PROGRESS = "batch-export-progress";
    CULLING_COMPLETE = "culling-complete";
    CULLING_PROGRESS = "culling-progress";
    CULLING_START = "culling-start";
    DENOISE_BATCH_PROGRESS = "denoise-batch-progress";
    DENOISE_COMPLETE = "denoise-complete";
    DENOISE_ERROR = "denoise-error";
    DENOISE_PROGRESS = "denoise-progress";
    EXPORT_CANCELLED = "export-cancelled";
    EXPORT_CANCELLING = "export-cancelling";
    EXPORT_COMPLETE = "export-complete";
    EXPORT_ERROR = "export-error";
    EXTERNAL_EDIT_SESSION = "external-edit-session";
    FOCUS_STACK_COMPLETE = "focus-stack-complete";
    FOCUS_STACK_PROGRESS = "focus-stack-progress";
    FOCUS_STACK_REPORT = "focus-stack-report";
    HDR_COMPLETE = "hdr-complete";
    HDR_PROGRESS = "hdr-progress";
    IMAGE_METADATA_LOADED = "image-metadata-loaded";
    INDEXING_ERROR = "indexing-error";
    INDEXING_FINISHED = "indexing-finished";
    INDEXING_PROGRESS = "indexing-progress";
    INDEXING_STARTED = "indexing-started";
    NEGATIVE_BATCH_PROGRESS = "negative-batch-progress";
    OPEN_WITH_FILE = "open-with-file";
    PANORAMA_COMPLETE = "panorama-complete";
    PANORAMA_ERROR = "panorama-error";
    PANORAMA_PROGRESS = "panorama-progress";
    PANORAMA_WARNING = "panorama-warning";
    PROVIDER_LOOK_EXPORT_MISSING = "provider-look-export-missing";
    SUPER_RESOLUTION_PROGRESS = "super-resolution-progress";
    THUMBNAIL_GENERATED = "thumbnail-generated";
    THUMBNAIL_GENERATION_COMPLETE = "thumbnail-generation-complete";
    THUMBNAIL_GENERATION_ERROR = "thumbnail-generation-error";
    THUMBNAIL_PROGRESS = "thumbnail-progress";
    WGPU_FRAME_READY = "wgpu-frame-ready";
}

#[cfg(test)]
mod tests {
    use super::ALL;
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                sources(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }

    /// The event each `.emit(` in the app names: a literal as it stands, a
    /// constant of this list by its name. The look provider's and the
    /// engine's constants are theirs and not read.
    fn emitted() -> BTreeSet<String> {
        let mut files = Vec::new();
        sources(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut files,
        );
        let mut found = BTreeSet::new();
        for file in files {
            let name = file.file_name().unwrap().to_string_lossy().into_owned();
            if name == "events.rs" || name.starts_with("lut_provider") || name == "engine_bridge.rs"
            {
                continue;
            }
            let text = std::fs::read_to_string(&file).unwrap();
            for part in text.split(".emit(").skip(1) {
                let first = part.trim_start();
                let event = if let Some(literal) = first.strip_prefix('"') {
                    literal.split('"').next().unwrap().to_string()
                } else {
                    let path: String = first
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == ':')
                        .collect();
                    let id = path.rsplit("::").next().unwrap();
                    match ALL.iter().find(|(known, _)| *known == id) {
                        Some((_, name)) => name.to_string(),
                        None => continue, // not an event call (a `.emit(` of something else)
                    }
                };
                found.insert(event);
            }
        }
        found
    }

    #[test]
    fn a_constant_is_its_event_in_capitals() {
        for (id, name) in ALL {
            assert_eq!(*id, name.to_uppercase().replace('-', "_"), "{name}");
        }
    }

    #[test]
    fn every_event_the_app_emits_is_listed() {
        let listed: BTreeSet<String> = ALL.iter().map(|(_, n)| n.to_string()).collect();
        let unlisted: Vec<_> = emitted().difference(&listed).cloned().collect();
        assert!(
            unlisted.is_empty(),
            "emitted but not in mods/events.rs: {unlisted:?}"
        );
    }

    #[test]
    fn every_listed_event_is_emitted_somewhere() {
        let found = emitted();
        let never: Vec<_> = ALL
            .iter()
            .filter(|(_, n)| !found.contains(*n))
            .map(|(_, n)| n)
            .collect();
        assert!(
            never.is_empty(),
            "listed in mods/events.rs but emitted nowhere: {never:?}"
        );
    }
}
