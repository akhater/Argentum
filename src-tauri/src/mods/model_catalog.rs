//! Every AI model Argentum can download, in one list. Ours.
//!
//! WHY A LIST OF OUR OWN
//!
//! Each AI feature downloads its own model the first time it is used, into
//! `app_data_dir()/models`, and nothing kept track of what had arrived: a few
//! uses in, that folder holds well over a gigabyte that the user never chose to
//! download and had no way to see or remove. Settings > Processing > AI Models reads
//! this list to show what is there and delete what is not wanted.
//!
//! ADDING A MODEL IS ONE ENTRY
//!
//! Append a `Model` to `CATALOGUE` below: the file name it is saved under, a
//! label, one plain-English line saying what it is for, the address it comes
//! from and its download size. Nothing else changes; the settings page lists
//! whatever is here, and deleting only ever touches names that are here.
//!
//! The feature that uses a model still downloads it itself. This list does not
//! fetch anything, so a model deleted here is fetched again by its feature the
//! next time that feature is used.
//!
//! UPSTREAM'S MODELS ARE COPIES
//!
//! RapidRAW's file names and addresses are private constants in their
//! `ai_processing.rs`. Making them `pub` would be an edit to their file for the
//! sake of a settings page, so they are copied here instead, and the tests below
//! read their file and fail when the copies drift — a renamed model, or a new
//! one upstream added that this list does not know about yet. That dependency is
//! registered as `model-manager` in `scripts/upstream-registry.mjs`.

/// Where a model lives inside the models folder.
#[derive(Clone, Copy, Debug)]
pub enum Stored {
    /// Plain files, each as `(file name, the address it is downloaded from)`.
    /// One entry may hold several files when they only work together.
    Files(&'static [(&'static str, &'static str)]),
    /// Every folder whose name starts with `prefix`, so a runtime left behind
    /// by an older version is found and removed with the current one.
    FoldersStartingWith {
        prefix: &'static str,
        /// Nothing downloads from here; the tests hold it to the code that does.
        #[cfg_attr(not(test), allow(dead_code))]
        url: &'static str,
    },
}

#[derive(Clone, Copy, Debug)]
pub struct Model {
    /// How the settings page names it when asking for a deletion. Never shown.
    pub id: &'static str,
    /// What the settings page calls it.
    pub label: &'static str,
    /// What it is for, in one plain-English line.
    pub purpose: &'static str,
    pub stored: Stored,
    /// Bytes downloaded the first time it is used, where known.
    pub download_bytes: Option<u64>,
    /// RapidRAW's code downloads it, so `stored` copies constants of theirs,
    /// and the tests check them against their file.
    #[cfg_attr(not(test), allow(dead_code))]
    pub upstream: bool,
    /// Listed even when it is not on disk. Off for things that only exist on
    /// some machines, like the Windows-only graphics card runtime.
    pub listed_when_missing: bool,
}

/// The DirectML runtime's id. `model_manager` asks `gpu_runtime` before
/// removing it: once pinned, it has to stay on disk until the app exits.
pub const GPU_RUNTIME: &str = "gpu-runtime";

/// Builds a RapidRAW-Models address at compile time, the way theirs are written.
macro_rules! rapidraw_url {
    ($file:literal) => {
        concat!(
            "https://huggingface.co/CyberTimon/RapidRAW-Models/resolve/main/",
            $file,
            "?download=true"
        )
    };
}

pub const CATALOGUE: &[Model] = &[
    // --- RapidRAW's: masks. Their code downloads all four of these together
    // the first time any AI mask is used, so deleting one brings it back with
    // the next AI mask of any kind.
    Model {
        id: "subject",
        label: "Subject selection",
        purpose: "Picks out the thing you draw a box around, for the Subject mask.",
        stored: Stored::Files(&[
            (
                "sam_vit_b_01ec64_encoder.onnx",
                rapidraw_url!("sam_vit_b_01ec64_encoder.onnx"),
            ),
            (
                "sam_vit_b_01ec64_decoder.onnx",
                rapidraw_url!("sam_vit_b_01ec64_decoder.onnx"),
            ),
        ]),
        download_bytes: Some(100_293_382 + 8_751_331),
        upstream: true,
        listed_when_missing: true,
    },
    Model {
        id: "foreground",
        label: "Foreground detection",
        purpose: "Finds the main subject on its own, for the Foreground mask.",
        stored: Stored::Files(&[("u2net.onnx", rapidraw_url!("u2net.onnx"))]),
        download_bytes: Some(175_997_641),
        upstream: true,
        listed_when_missing: true,
    },
    Model {
        id: "sky",
        label: "Sky detection",
        purpose: "Finds the sky, for the Sky mask.",
        // Saved under an underscore, downloaded from a name with a hyphen.
        stored: Stored::Files(&[("skyseg_u2net.onnx", rapidraw_url!("skyseg-u2net.onnx"))]),
        download_bytes: Some(175_997_079),
        upstream: true,
        listed_when_missing: true,
    },
    Model {
        id: "depth",
        label: "Depth estimation",
        purpose: "Works out how far away each part of the photo is, for the Depth mask.",
        stored: Stored::Files(&[(
            "depth_anything_v2_vits.onnx",
            rapidraw_url!("depth_anything_v2_vits.onnx"),
        )]),
        download_bytes: Some(99_373_606),
        upstream: true,
        listed_when_missing: true,
    },
    // --- RapidRAW's: one feature each.
    Model {
        id: "inpainting",
        label: "Inpainting",
        purpose: "Fills in the area you erase with Inpainting.",
        stored: Stored::Files(&[("lama_fp16.onnx", rapidraw_url!("lama_fp16.onnx"))]),
        download_bytes: Some(111_545_583),
        upstream: true,
        listed_when_missing: true,
    },
    Model {
        id: "denoise",
        label: "AI noise reduction",
        purpose: "Cleans up noise when you use Denoise Image.",
        stored: Stored::Files(&[(
            "nind_denoise_utnet_684.onnx",
            rapidraw_url!("nind_denoise_utnet_684.onnx"),
        )]),
        download_bytes: Some(124_202_581),
        upstream: true,
        listed_when_missing: true,
    },
    Model {
        id: "tagging",
        label: "Photo tagging",
        purpose: "Tags photos by what is in them so a folder can be searched. Only used when AI Tagging is on.",
        stored: Stored::Files(&[
            ("clip_model.onnx", rapidraw_url!("clip_model.onnx")),
            ("clip_tokenizer.json", rapidraw_url!("clip_tokenizer.json")),
        ]),
        download_bytes: Some(605_804_513 + 2_224_041),
        upstream: true,
        listed_when_missing: true,
    },
    // --- Argentum's.
    Model {
        id: "super-resolution-2x",
        label: "Super Resolution 2x",
        purpose: "Enlarges a photo to twice its size.",
        stored: Stored::Files(&[(
            "realesrgan_x2plus.onnx",
            "https://huggingface.co/fernandotonon/QtMeshEditor-realesrgan-onnx/resolve/main/RealESRGAN_x2plus.onnx?download=true",
        )]),
        download_bytes: Some(67_156_218),
        upstream: false,
        listed_when_missing: true,
    },
    Model {
        id: "super-resolution-4x",
        label: "Super Resolution 4x",
        purpose: "Enlarges a photo to four times its size.",
        stored: Stored::Files(&[(
            "realesrgan_x4plus.onnx",
            "https://huggingface.co/mhmtaufiq/realesrgan-onnx/resolve/main/RealESRGAN_x4plus.onnx?download=true",
        )]),
        download_bytes: Some(68_811_310),
        upstream: false,
        listed_when_missing: true,
    },
    Model {
        id: "object-edge",
        label: "Object mask edges",
        purpose: "Draws the exact edge of an Object mask: hair, a bracelet against a wrist, the frame of a lamp.",
        stored: Stored::Files(&[(
            "vitmatte_small_composition_1k.onnx",
            "https://huggingface.co/Xenova/vitmatte-small-composition-1k/resolve/main/onnx/model.onnx?download=true",
        )]),
        download_bytes: Some(103_885_865),
        upstream: false,
        listed_when_missing: true,
    },
    Model {
        id: GPU_RUNTIME,
        label: "Graphics card support",
        purpose: "Lets Super Resolution run on the graphics card, many times faster than without it. Windows only.",
        stored: Stored::FoldersStartingWith {
            prefix: "onnxruntime-directml-",
            url: "https://api.nuget.org/v3-flatcontainer/microsoft.ml.onnxruntime.directml/1.22.0/microsoft.ml.onnxruntime.directml.1.22.0.nupkg",
        },
        download_bytes: Some(17_898_472),
        upstream: false,
        listed_when_missing: false,
    },
];

/// The catalogue entry with this id.
pub fn find(id: &str) -> Option<&'static Model> {
    CATALOGUE.iter().find(|m| m.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    const THEIR_AI: &str = include_str!("../ai_processing.rs");
    const OUR_SUPER_RESOLUTION: &str = include_str!("super_resolution.rs");
    const OUR_GPU_RUNTIME: &str = include_str!("gpu_runtime.rs");
    const OUR_MATTING: &str = include_str!("matting.rs");

    fn files(model: &Model) -> &'static [(&'static str, &'static str)] {
        match model.stored {
            Stored::Files(files) => files,
            Stored::FoldersStartingWith { .. } => &[],
        }
    }

    /// The value of every `const NAME_FILENAME: &str = "...";` in their file.
    fn their_filename_constants() -> Vec<(String, String)> {
        THEIR_AI
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                let rest = line.strip_prefix("const ")?;
                let (name, value) = rest.split_once(": &str = \"")?;
                if !name.ends_with("_FILENAME") {
                    return None;
                }
                let value = value.strip_suffix("\";")?;
                Some((name.to_string(), value.to_string()))
            })
            .collect()
    }

    #[test]
    fn ids_are_unique_and_every_entry_names_something() {
        let mut seen = HashSet::new();
        for model in CATALOGUE {
            assert!(seen.insert(model.id), "duplicate id {}", model.id);
            assert!(!model.label.is_empty() && !model.purpose.is_empty());
            match model.stored {
                Stored::Files(files) => assert!(!files.is_empty(), "{} stores nothing", model.id),
                Stored::FoldersStartingWith { prefix, .. } => {
                    assert!(!prefix.is_empty(), "{} matches every folder", model.id)
                }
            }
        }
    }

    /// Deleting joins these onto the models folder; a separator or `..` here
    /// would reach outside it. `model_manager` refuses those too, but a list
    /// that never contains one is the first line.
    #[test]
    fn every_name_is_a_bare_file_name() {
        for model in CATALOGUE {
            let names: Vec<&str> = match model.stored {
                Stored::Files(files) => files.iter().map(|(name, _)| *name).collect(),
                Stored::FoldersStartingWith { prefix, .. } => vec![prefix],
            };
            for name in names {
                assert!(
                    super::super::model_manager::is_bare_name(name),
                    "{} names {name:?}",
                    model.id
                );
            }
        }
    }

    /// The copies of RapidRAW's constants are still what their code uses. A
    /// rename upstream would otherwise leave a model their code downloads that
    /// this list cannot see, and a row here for a file that never arrives.
    #[test]
    fn upstream_names_and_addresses_match_their_constants() {
        for model in CATALOGUE.iter().filter(|m| m.upstream) {
            for (name, url) in files(model) {
                assert!(
                    THEIR_AI.contains(&format!("\"{name}\"")),
                    "{}: {name} is no longer a file name in ai_processing.rs",
                    model.id
                );
                assert!(
                    THEIR_AI.contains(&format!("\"{url}\"")),
                    "{}: {url} is no longer an address in ai_processing.rs",
                    model.id
                );
            }
        }
    }

    /// The other direction: every model file their code saves is in this list.
    /// When upstream adds a model, this fails until it has an entry, so the
    /// settings page never quietly under-reports what is on disk.
    #[test]
    fn every_model_their_code_saves_is_listed() {
        let ours: HashSet<&str> = CATALOGUE
            .iter()
            .filter(|m| m.upstream)
            .flat_map(|m| files(m).iter().map(|(name, _)| *name))
            .collect();
        let theirs = their_filename_constants();
        assert!(
            theirs.len() >= 10,
            "found only {theirs:?}; has the file's layout changed?"
        );
        for (constant, value) in theirs {
            // A name their code renames to the current one on first use.
            if constant.contains("LEGACY") {
                continue;
            }
            assert!(
                ours.contains(value.as_str()),
                "{constant} = {value:?} in ai_processing.rs has no entry in CATALOGUE"
            );
        }
    }

    /// Ours are copies too, of `super_resolution`, `matting` and `gpu_runtime`.
    #[test]
    fn our_names_and_addresses_match_the_features_that_download_them() {
        for (name, url) in files(find("object-edge").expect("listed")) {
            assert!(
                OUR_MATTING.contains(&format!("\"{name}\"")),
                "object-edge: {name}"
            );
            assert!(
                OUR_MATTING.contains(&format!("\"{url}\"")),
                "object-edge: {url}"
            );
        }
        for id in ["super-resolution-2x", "super-resolution-4x"] {
            let model = find(id).expect("listed");
            for (name, url) in files(model) {
                assert!(
                    OUR_SUPER_RESOLUTION.contains(&format!("\"{name}\"")),
                    "{id}: {name}"
                );
                assert!(
                    OUR_SUPER_RESOLUTION.contains(&format!("\"{url}\"")),
                    "{id}: {url}"
                );
            }
        }
        let Stored::FoldersStartingWith { prefix, url } = find(GPU_RUNTIME).expect("listed").stored
        else {
            panic!("the graphics card runtime is a folder");
        };
        assert!(OUR_GPU_RUNTIME.contains(&format!("\"{prefix}")), "{prefix}");
        assert!(OUR_GPU_RUNTIME.contains(&format!("\"{url}\"")), "{url}");
    }
}
