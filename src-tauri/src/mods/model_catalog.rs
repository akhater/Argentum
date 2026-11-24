//! Every AI model Argentum can download, in one list. Ours.
//!
//! WHY A LIST OF OUR OWN
//!
//! Each AI feature downloads its own model the first time it is used, into
//! `app_data_dir()/models`, and nothing kept track of what had arrived: a few
//! uses in, that folder holds well over a gigabyte that the user never chose to
//! download and had no way to see or remove. Settings > General > AI Models reads
//! this list to show what is there, download what is not, and delete what is
//! not wanted.
//!
//! ADDING A MODEL IS ONE ENTRY
//!
//! Append a `Model` to `CATALOGUE` below: the files it is saved as (name,
//! address, SHA-256 and size), a label, and one plain-English line saying what
//! it is for. Nothing else changes; the settings page lists whatever is here,
//! downloads exactly these files, and deleting only ever touches names that
//! are here. A file published by someone else is first uploaded, unchanged,
//! to our release of copies (`our_copy!`, see `mirror`): that is its address,
//! and the original is its fallback.
//!
//! The feature that uses a model asks `model_download::ensure` for it, which
//! fetches whatever of its files is missing or wrong, as the settings page's
//! Download does: one list, one downloader.
//!
//! NEVER A NEW NAME FOR A FILE USERS HAVE
//!
//! Users already have gigabytes of these files. A file is kept only when its
//! name and checksum match, so renaming one, or moving it to another folder,
//! downloads it again for everyone. A test
//! (`files_users_already_have_keep_their_names_and_checksums`) holds this list
//! to every file an earlier version saved, and `RENAMED` keeps an old name of
//! RapidRAW's in use.
//!
//! The selection models' list is `rm_selection::manifest`, read through
//! `selection_models::files`.

use super::mirror::mirror;

/// One file of a model: where the feature that uses it saves it, fetches it
/// from, and what it checks it against.
#[derive(Clone, Copy, Debug)]
pub struct File {
    /// Its name in the models folder, or in the entry's own folder.
    pub name: &'static str,
    pub url: &'static str,
    /// Tried when `url` cannot be reached: the same bytes, so the same checksum.
    pub fallback: Option<&'static str>,
    pub sha256: &'static str,
    pub bytes: u64,
}

/// Where a model lives inside the models folder.
#[derive(Clone, Copy, Debug)]
pub enum Stored {
    /// Plain files directly in the models folder. One entry may hold several
    /// files when they only work together.
    Files(&'static [File]),
    /// Files in a folder of their own inside the models folder, listed by the
    /// code that uses them rather than copied here.
    Folder {
        name: &'static str,
        files: fn() -> Vec<File>,
    },
    /// Every folder whose name starts with `prefix`, so a runtime left behind
    /// by an older version is found and removed with the current one. Not
    /// downloaded from the settings page: the feature that needs it installs it.
    FoldersStartingWith { prefix: &'static str, bytes: u64 },
}

impl Stored {
    /// The files to download, by name and checksum. None for a runtime folder.
    pub fn files(&self) -> Vec<File> {
        match *self {
            Stored::Files(files) => files.to_vec(),
            Stored::Folder { files, .. } => files(),
            Stored::FoldersStartingWith { .. } => Vec::new(),
        }
    }

    /// The entry's own folder inside the models folder, if it has one.
    pub fn folder(&self) -> Option<&'static str> {
        match *self {
            Stored::Folder { name, .. } => Some(name),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Model {
    /// How the settings page names it when asking for a download or deletion.
    /// Never shown.
    pub id: &'static str,
    /// What the settings page calls it.
    pub label: &'static str,
    /// What it is for, in one plain-English line.
    pub purpose: &'static str,
    pub stored: Stored,
    /// Listed even when it is not on disk. Off for things that only exist on
    /// some machines, like the Windows-only graphics card runtime.
    pub listed_when_missing: bool,
}

/// The DirectML runtime's id. `model_manager` asks `gpu_runtime` before
/// removing it: once pinned, it has to stay on disk until the app exits.
pub const GPU_RUNTIME: &str = "gpu-runtime";

/// The selection models' id: `selection_models` downloads under it too.
pub const SELECTION: &str = "selection";

/// Our release holding copies of other projects' models (`mirror`): each
/// under the name its original address publishes it as.
macro_rules! our_copy {
    ($published:literal) => {
        mirror!("models-mirror-v1", $published)
    };
}

/// One of RapidRAW-Models' files, saved as `$saved` and published as
/// `$published`: from our copy, then from theirs, written the way their
/// addresses are.
macro_rules! rapidraw {
    ($saved:literal, $published:literal, $sha256:literal, $bytes:expr) => {
        File {
            name: $saved,
            url: our_copy!($published),
            fallback: Some(concat!(
                "https://huggingface.co/CyberTimon/RapidRAW-Models/resolve/main/",
                $published,
                "?download=true"
            )),
            sha256: $sha256,
            bytes: $bytes,
        }
    };
}

/// The Text mask's id: `text_mask` downloads under it too.
pub const TEXT_MASKS: &str = "text-masks";

/// CLIP's word list. Photo tagging and the Text mask both read this one file.
pub const CLIP_TOKENIZER: File = rapidraw!(
    "clip_tokenizer.json",
    "clip_tokenizer.json",
    // RapidRAW's code checked no checksum for the tokenizer. This one is ours:
    // the file Hugging Face served on 2026-10-21 (git blob 564c0ebd, the one
    // their listing names, unchanged since it was uploaded on 2025-07-27).
    "b556ac8c99757ffb677208af34bc8c6721572114111a6e0aaf5fa69ff0b8d842",
    2_224_041
);

/// The Face mask's id: `face_mask` downloads under it too.
pub const FACE_MASKS: &str = "face-masks";

// The Text and Face masks run on RapidRAW 1.6.7's files, under the names,
// addresses and checksums they give them (ai_processing.rs @ 211ef559 and
// 07ef9211, `ESAM3_*`, `FACE_*` and `AiMaskModel`). The inherited 1.6.6 code
// here never saves these, so the copies are held to `text_mask` and
// `face_mask`, which download them.

/// EfficientSAM3's image encoder and decoder: the Text and Face masks' shared part.
const ESAM3_IMAGE_ENCODER: File = rapidraw!(
    "efficientsam3_ev_m_image_encoder.onnx",
    "efficientsam3_ev_m_image_encoder.onnx",
    "2ed08306579138120dbf9f1110fd31784d1c20fe1b67fa2187a0a2a6d7c82729",
    85_436_582
);
const ESAM3_DECODER: File = rapidraw!(
    "efficientsam3_ev_m_decoder.onnx",
    "efficientsam3_ev_m_decoder.onnx",
    "16accc22a07ee99c73ef8a14e286ec567f94c116e0d8efa1a3a6c0bc9a2596a8",
    129_858_624
);

/// What the Face mask runs on: EfficientSAM3 with a stored "face" prompt
/// finds the faces, MediaPipe's landmarks and selfie segmenter draw the parts.
pub const FACE_MASK_FILES: &[File] = &[
    ESAM3_IMAGE_ENCODER,
    ESAM3_DECODER,
    rapidraw!(
        "efficientsam3_face_prompt.bin",
        "efficientsam3_face_prompt.bin",
        "8fdbe63ef853756b1da9cdc642c277a6e422452f3feaf35f4297e6aea3a1ffb9",
        16_448
    ),
    rapidraw!(
        "mediapipe_face_landmarks.onnx",
        "mediapipe_face_landmarks.onnx",
        "6524d596b4d9151b0d8d590844566261f21de5186ac38125bdcd1de6974b0bb4",
        4_920_998
    ),
    rapidraw!(
        "mediapipe_selfie_multiclass.onnx",
        "mediapipe_selfie_multiclass.onnx",
        "a198470a4edec350a33b8182c149e417effc5e61c0506b947f226e7a81b9a08e",
        16_454_469
    ),
];

/// What the Text mask runs on.
pub const TEXT_MASK_FILES: &[File] = &[
    ESAM3_IMAGE_ENCODER,
    ESAM3_DECODER,
    rapidraw!(
        "efficientsam3_ev_m_text_encoder.onnx",
        "efficientsam3_ev_m_text_encoder.onnx",
        "3065ea6116f21f6e22bc70a0c041e5da9ae05fdec7b48df953394f5657609590",
        169_171_689
    ),
    CLIP_TOKENIZER,
];

/// RapidRAW's models' ids and files: `ai_processing` loads them under these
/// names, the names and checksums its own code saved them with (1.6.6).
pub const SUBJECT: &str = "subject";
pub const FOREGROUND: &str = "foreground";
pub const SKY: &str = "sky";
pub const DEPTH: &str = "depth";
pub const RELIGHT: &str = "relight";
pub const INPAINTING: &str = "inpainting";
pub const DENOISE: &str = "denoise";
pub const TAGGING: &str = "tagging";

pub const SAM_ENCODER: File = rapidraw!(
    "sam_vit_b_01ec64_encoder.onnx",
    "sam_vit_b_01ec64_encoder.onnx",
    "16ab73d9c824886f0de2938c19df22fb9ec3deebfd0de58e65177e479213d7d1",
    100_293_382
);
pub const SAM_DECODER: File = rapidraw!(
    "sam_vit_b_01ec64_decoder.onnx",
    "sam_vit_b_01ec64_decoder.onnx",
    "85d0d672cf5b7fe763edcde429e5533e62f674af4b15c7d688b7673b0ef00bf7",
    8_751_331
);
pub const U2NET: File = rapidraw!(
    "u2net.onnx",
    "u2net.onnx",
    "8d10d2f3bb75ae3b6d527c77944fc5e7dcd94b29809d47a739a7a728a912b491",
    175_997_641
);
pub const SKYSEG: File = rapidraw!(
    "skyseg_u2net.onnx",
    "skyseg-u2net.onnx",
    "ab9c34c64c3d821220a2886a4a06da4642ffa14d5b30e8d5339056a089aa1d39",
    175_997_079
);
pub const DEPTH_ANYTHING: File = rapidraw!(
    "depth_anything_v2_vits.onnx",
    "depth_anything_v2_vits.onnx",
    "d2b11a11c1d4a12b47608fa65a17ee9a4c605b55ee1730c8e3b526304f2562be",
    99_373_606
);
pub const MOGE_NORMAL: File = rapidraw!(
    "moge-2-vits-normal-onnx.onnx",
    "moge-2-vits-normal-onnx.onnx",
    "24eacb5dc7a2c54c7bc98f7de085ffbed79ad006ea5b664c2c2cdc02ff3a52f0",
    140_852_051
);
pub const LAMA: File = rapidraw!(
    "lama_fp16.onnx",
    "lama_fp16.onnx",
    "2d6be6277c400d6f1b91819737f7c3da935e5c63d1b521d393be1196a2bfa82c",
    111_545_583
);
pub const NIND_DENOISE: File = rapidraw!(
    "nind_denoise_utnet_684.onnx",
    "nind_denoise_utnet_684.onnx",
    "ee3586279d514df557ff3f7dec6df37fafc51ba5d3a3435b2cc9ac2d9017e7fe",
    124_202_581
);
pub const CLIP_MODEL: File = rapidraw!(
    "clip_model.onnx",
    "clip_model.onnx",
    "57879bb1c23cdeb350d23569dd251ed4b740a96d747c529e94a2bb8040ac5d00",
    605_804_513
);

/// Super Resolution's and Restore's ids and files: `super_resolution` loads
/// them under these names.
pub const SUPER_RESOLUTION_2X: &str = "super-resolution-2x";
pub const SUPER_RESOLUTION_4X: &str = "super-resolution-4x";
pub const RESTORE: &str = "restore";

/// The published Real-ESRGAN x2plus ONNX artifact.
pub const REALESRGAN_X2: File = File {
    name: "realesrgan_x2plus.onnx",
    url: our_copy!("RealESRGAN_x2plus.onnx"),
    fallback: Some(
        "https://huggingface.co/fernandotonon/QtMeshEditor-realesrgan-onnx/resolve/main/RealESRGAN_x2plus.onnx?download=true",
    ),
    sha256: "35d016524a6eb7e9a99f192cfa81b6cf1f80fdf8f1b51605d77f0763f5ccba7f",
    bytes: 67_156_218,
};
/// The published Real-ESRGAN x4plus ONNX artifact.
pub const REALESRGAN_X4: File = File {
    name: "realesrgan_x4plus.onnx",
    url: our_copy!("RealESRGAN_x4plus.onnx"),
    fallback: Some(
        "https://huggingface.co/mhmtaufiq/realesrgan-onnx/resolve/main/RealESRGAN_x4plus.onnx?download=true",
    ),
    sha256: "3767c17388381cfca3d7196a4a517737fefc22b57c9fd98d3bae78e98e2bebc9",
    bytes: 68_811_310,
};
/// Argentum's ONNX export of OmniLens's SwinIR_AODLib_EAOD.pth: our copy
/// is the only one.
pub const OMNILENS_RESTORE: File = File {
    name: "omnilens_swinir_aodlib_256.onnx",
    url: mirror!("omnilens-v1", "omnilens_swinir_aodlib_256.onnx"),
    fallback: None,
    sha256: "c28ec5478a6db236e9151961ad1afe29ea23b80fecc8612a436f329e9a0e9630",
    bytes: 53_592_179,
};

pub const CATALOGUE: &[Model] = &[
    // --- RAWmakase's, through `selection_models`: the masks the Masks menu
    // makes since PRs #103 and #104.
    Model {
        id: SELECTION,
        label: "Selection masks",
        purpose: "Finds people, animals and the sky, and picks out what you click, for the Subject, Object, Sky and Background masks.",
        stored: Stored::Folder {
            name: super::selection_models::FOLDER,
            files: super::selection_models::files,
        },
        listed_when_missing: true,
    },
    // --- RapidRAW's: their code downloads these four together
    // (`get_or_init_ai_models`) the first time Foreground, Depth, Lens blur,
    // Fog, Quick Erase or Inpainting's Subject is used, so deleting one brings
    // it back with the next of those: all four are in use. Their Subject
    // (SAM 1) and Sky left the Masks menu for the selection masks above; their
    // Sky is still loaded with the others and has to be there for any of
    // them. Loading it only for the Sky mask would save its 168 MB, but that
    // is a change to their loader, not to this list.
    Model {
        id: SUBJECT,
        label: "Subject selection",
        // Also redraws the box of a Subject mask made before the selection
        // masks: the mask is kept in the edit, and only a new box runs this.
        purpose: "Picks out the thing you draw a box around, for Quick Erase and Subject in Inpainting. The Masks menu no longer uses it.",
        stored: Stored::Files(&[SAM_ENCODER, SAM_DECODER]),
        listed_when_missing: true,
    },
    Model {
        id: FOREGROUND,
        label: "Foreground detection",
        purpose: "Finds the main subject on its own, for the Foreground mask.",
        stored: Stored::Files(&[U2NET]),
        listed_when_missing: true,
    },
    Model {
        id: SKY,
        label: "Sky detection",
        purpose: "Loaded with Foreground and Depth, which need it to be here. The Sky mask itself now uses Selection masks.",
        // Saved under an underscore, downloaded from a name with a hyphen.
        stored: Stored::Files(&[SKYSEG]),
        listed_when_missing: true,
    },
    Model {
        id: DEPTH,
        label: "Depth estimation",
        purpose: "Works out how far away each part of the photo is, for the Depth mask, Lens blur and Fog.",
        stored: Stored::Files(&[DEPTH_ANYTHING]),
        listed_when_missing: true,
    },
    // --- RapidRAW's: one feature each.
    Model {
        id: RELIGHT,
        label: "Surface directions",
        purpose: "Works out which way each surface faces, for Relight.",
        stored: Stored::Files(&[MOGE_NORMAL]),
        listed_when_missing: true,
    },
    Model {
        id: INPAINTING,
        label: "Inpainting",
        purpose: "Fills in the area you erase with Inpainting.",
        stored: Stored::Files(&[LAMA]),
        listed_when_missing: true,
    },
    Model {
        id: DENOISE,
        label: "AI noise reduction",
        purpose: "Cleans up noise when you use Denoise Image.",
        stored: Stored::Files(&[NIND_DENOISE]),
        listed_when_missing: true,
    },
    Model {
        id: TAGGING,
        label: "Photo tagging",
        purpose: "Tags photos by what is in them so a folder can be searched. Only used when AI Tagging is on.",
        stored: Stored::Files(&[CLIP_MODEL, CLIP_TOKENIZER]),
        listed_when_missing: true,
    },
    // --- RapidRAW 1.6.7's, run by our code (`text_mask`, `face_mask`).
    Model {
        id: TEXT_MASKS,
        label: "Text masks",
        purpose: "Picks out what you type, such as car or water, for the Text mask.",
        stored: Stored::Files(TEXT_MASK_FILES),
        listed_when_missing: true,
    },
    Model {
        id: FACE_MASKS,
        label: "Face masks",
        purpose: "Finds every face and picks out the parts you choose (skin, hair, eyes, lips and more) for the Face mask.",
        stored: Stored::Files(FACE_MASK_FILES),
        listed_when_missing: true,
    },
    // --- Argentum's.
    Model {
        id: SUPER_RESOLUTION_2X,
        label: "Super Resolution 2x",
        purpose: "Enlarges a photo to twice its size.",
        stored: Stored::Files(&[REALESRGAN_X2]),
        listed_when_missing: true,
    },
    Model {
        id: SUPER_RESOLUTION_4X,
        label: "Super Resolution 4x",
        purpose: "Enlarges a photo to four times its size.",
        stored: Stored::Files(&[REALESRGAN_X4]),
        listed_when_missing: true,
    },
    Model {
        id: RESTORE,
        label: "Restore",
        purpose: "Undoes lens softness, such as a lens used wide open, at the photo's own size. Used by Restore in the Super Resolution window.",
        stored: Stored::Files(&[OMNILENS_RESTORE]),
        listed_when_missing: true,
    },
    Model {
        id: GPU_RUNTIME,
        label: "Graphics card support",
        purpose: "Lets Super Resolution and Restore run on the graphics card, many times faster than without it. Windows only.",
        stored: Stored::FoldersStartingWith {
            prefix: "onnxruntime-directml-",
            bytes: super::gpu_runtime::PACKAGE.bytes,
        },
        listed_when_missing: false,
    },
];

/// Files an earlier version downloaded for a feature that has since been
/// removed. Nothing lists them, so nothing would ever offer to delete them:
/// `model_manager` removes them at the next start instead.
///
/// - `vitmatte_small_composition_1k.onnx`, about 100 MB: the Object mask's edge
///   model, 26.41.4 only. The Object mask lost to the Subject mask's box.
pub const RETIRED: &[&str] = &["vitmatte_small_composition_1k.onnx"];

/// `(old name, current name)`: a model saved under an old name, renamed to the
/// current one before anything would download it (`model_download`, as
/// RapidRAW's `promote_legacy_model_filename` did). Kept while the current one
/// is missing, since it is that model; a leftover once the current one is there.
pub const RENAMED: &[(&str, &str)] = &[("skyseg-u2net.onnx", "skyseg_u2net.onnx")];

/// The catalogue entry with this id.
pub fn find(id: &str) -> Option<&'static Model> {
    CATALOGUE.iter().find(|m| m.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn ids_are_unique_and_every_entry_names_something() {
        let mut seen = HashSet::new();
        for model in CATALOGUE {
            assert!(seen.insert(model.id), "duplicate id {}", model.id);
            assert!(!model.label.is_empty() && !model.purpose.is_empty());
            match model.stored {
                Stored::FoldersStartingWith { prefix, .. } => {
                    assert!(!prefix.is_empty(), "{} matches every folder", model.id)
                }
                stored => assert!(!stored.files().is_empty(), "{} stores nothing", model.id),
            }
        }
    }

    /// Deleting joins these onto the models folder; a separator or `..` here
    /// would reach outside it. `model_manager` refuses those too, but a list
    /// that never contains one is the first line.
    #[test]
    fn every_name_is_a_bare_file_name() {
        let bare = super::super::model_manager::is_bare_name;
        for model in CATALOGUE {
            let mut names: Vec<&str> = model.stored.files().iter().map(|f| f.name).collect();
            names.extend(model.stored.folder());
            if let Stored::FoldersStartingWith { prefix, .. } = model.stored {
                names.push(prefix);
            }
            for name in names {
                assert!(bare(name), "{} names {name:?}", model.id);
            }
        }
        for (old, current) in RENAMED {
            assert!(bare(old) && bare(current), "{old:?} -> {current:?}");
        }
    }

    /// Every file a download writes is checked against a pinned SHA-256, and
    /// comes from an address that cannot quietly change protocol.
    #[test]
    fn every_file_is_pinned() {
        for model in CATALOGUE {
            for file in model.stored.files() {
                assert_eq!(file.sha256.len(), 64, "{}", file.name);
                assert!(
                    file.sha256
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
                    "{}",
                    file.name
                );
                assert!(file.bytes > 0, "{}", file.name);
                for url in std::iter::once(file.url).chain(file.fallback) {
                    assert!(url.starts_with("https://"), "{url}");
                }
            }
        }
    }

    /// A feature never depends on one server: every file has a second address,
    /// tried when the first fails. Only Restore's model has no home but our
    /// copy, because Argentum made it.
    #[test]
    fn every_file_has_a_second_address() {
        for model in CATALOGUE {
            for file in model.stored.files() {
                assert!(
                    file.fallback.is_some() || file.name == OMNILENS_RESTORE.name,
                    "{} has one address",
                    file.name
                );
            }
        }
    }

    /// What RapidRAW and Real-ESRGAN publish comes from our copy first, and
    /// from theirs only when ours fails. The selection models come from
    /// RAWmakase's own copy first (`rm_selection::manifest`).
    #[test]
    fn files_we_keep_a_copy_of_come_from_it_first() {
        for model in CATALOGUE.iter().filter(|m| m.id != SELECTION) {
            for file in model.stored.files() {
                assert!(
                    file.url.starts_with(mirror!()),
                    "{}: {}",
                    file.name,
                    file.url
                );
            }
        }
    }

    /// Each old name is renamed to a name a listed model uses.
    #[test]
    fn renamed_files_become_listed_ones() {
        for (old, current) in RENAMED {
            assert!(
                CATALOGUE
                    .iter()
                    .any(|m| m.stored.files().iter().any(|f| f.name == *current)),
                "{old} becomes {current}, which nothing lists"
            );
        }
    }

    /// A retired name is deleted without asking, so it must be a bare name and
    /// must never be one a listed model still uses.
    #[test]
    fn retired_names_are_bare_and_belong_to_nothing_listed() {
        for name in RETIRED {
            assert!(super::super::model_manager::is_bare_name(name), "{name:?}");
            for model in CATALOGUE {
                let taken = match model.stored {
                    Stored::FoldersStartingWith { prefix, .. } => name.starts_with(prefix),
                    stored => {
                        stored.folder() == Some(*name)
                            || stored.files().iter().any(|f| f.name == *name)
                    }
                };
                assert!(!taken, "{name} is retired and still used by {}", model.id);
            }
        }
    }

    /// The graphics card runtime is found by the folder `gpu_runtime` installs it in.
    #[test]
    fn the_gpu_runtime_entry_finds_the_folder_it_is_installed_in() {
        let Stored::FoldersStartingWith { prefix, .. } = find(GPU_RUNTIME).expect("listed").stored
        else {
            panic!("the graphics card runtime is a folder");
        };
        assert!(super::super::gpu_runtime::FOLDER.starts_with(prefix));
    }

    /// Every file an earlier version saved straight into the models folder,
    /// under the name and with the SHA-256 it was saved with. Users have
    /// gigabytes of these, and the downloader keeps a file only when its name
    /// and checksum match, so a list that renamed one or changed its checksum
    /// would fetch it all again. Copied from the code that saved them, before
    /// it asked this list: RapidRAW's `ai_processing.rs` constants (1.6.6),
    /// `super_resolution.rs` (26.44.1), and the Text and Face masks (#215,
    /// #216). Add to it; never change a line.
    const SAVED_BY_EARLIER_VERSIONS: &[(&str, &str)] = &[
        (
            "sam_vit_b_01ec64_encoder.onnx",
            "16ab73d9c824886f0de2938c19df22fb9ec3deebfd0de58e65177e479213d7d1",
        ),
        (
            "sam_vit_b_01ec64_decoder.onnx",
            "85d0d672cf5b7fe763edcde429e5533e62f674af4b15c7d688b7673b0ef00bf7",
        ),
        (
            "u2net.onnx",
            "8d10d2f3bb75ae3b6d527c77944fc5e7dcd94b29809d47a739a7a728a912b491",
        ),
        (
            "skyseg_u2net.onnx",
            "ab9c34c64c3d821220a2886a4a06da4642ffa14d5b30e8d5339056a089aa1d39",
        ),
        (
            "clip_model.onnx",
            "57879bb1c23cdeb350d23569dd251ed4b740a96d747c529e94a2bb8040ac5d00",
        ),
        (
            "nind_denoise_utnet_684.onnx",
            "ee3586279d514df557ff3f7dec6df37fafc51ba5d3a3435b2cc9ac2d9017e7fe",
        ),
        (
            "lama_fp16.onnx",
            "2d6be6277c400d6f1b91819737f7c3da935e5c63d1b521d393be1196a2bfa82c",
        ),
        (
            "depth_anything_v2_vits.onnx",
            "d2b11a11c1d4a12b47608fa65a17ee9a4c605b55ee1730c8e3b526304f2562be",
        ),
        (
            "moge-2-vits-normal-onnx.onnx",
            "24eacb5dc7a2c54c7bc98f7de085ffbed79ad006ea5b664c2c2cdc02ff3a52f0",
        ),
        (
            "realesrgan_x2plus.onnx",
            "35d016524a6eb7e9a99f192cfa81b6cf1f80fdf8f1b51605d77f0763f5ccba7f",
        ),
        (
            "realesrgan_x4plus.onnx",
            "3767c17388381cfca3d7196a4a517737fefc22b57c9fd98d3bae78e98e2bebc9",
        ),
        (
            "omnilens_swinir_aodlib_256.onnx",
            "c28ec5478a6db236e9151961ad1afe29ea23b80fecc8612a436f329e9a0e9630",
        ),
        (
            "efficientsam3_ev_m_image_encoder.onnx",
            "2ed08306579138120dbf9f1110fd31784d1c20fe1b67fa2187a0a2a6d7c82729",
        ),
        (
            "efficientsam3_ev_m_decoder.onnx",
            "16accc22a07ee99c73ef8a14e286ec567f94c116e0d8efa1a3a6c0bc9a2596a8",
        ),
        (
            "efficientsam3_ev_m_text_encoder.onnx",
            "3065ea6116f21f6e22bc70a0c041e5da9ae05fdec7b48df953394f5657609590",
        ),
        (
            "efficientsam3_face_prompt.bin",
            "8fdbe63ef853756b1da9cdc642c277a6e422452f3feaf35f4297e6aea3a1ffb9",
        ),
        (
            "mediapipe_face_landmarks.onnx",
            "6524d596b4d9151b0d8d590844566261f21de5186ac38125bdcd1de6974b0bb4",
        ),
        (
            "mediapipe_selfie_multiclass.onnx",
            "a198470a4edec350a33b8182c149e417effc5e61c0506b947f226e7a81b9a08e",
        ),
    ];

    #[test]
    fn files_users_already_have_keep_their_names_and_checksums() {
        let listed: Vec<File> = CATALOGUE
            .iter()
            .filter(|m| m.stored.folder().is_none())
            .flat_map(|m| m.stored.files())
            .collect();
        for (name, sha256) in SAVED_BY_EARLIER_VERSIONS {
            assert!(
                listed
                    .iter()
                    .any(|f| f.name == *name && f.sha256 == *sha256),
                "{name} with its checksum is no longer in the catalogue: users would download it again"
            );
        }
        // RapidRAW's code saved the sky model under a hyphen before 1.6.6;
        // that file is still renamed rather than fetched again.
        assert!(RENAMED.contains(&("skyseg-u2net.onnx", "skyseg_u2net.onnx")));
    }

    /// The selection entry is the selection feature's own list, in its folder.
    #[test]
    fn the_selection_entry_is_what_selection_models_reads() {
        let stored = find(SELECTION).expect("listed").stored;
        assert_eq!(
            stored.folder(),
            Some(super::super::selection_models::FOLDER)
        );
        let names: Vec<&str> = stored.files().iter().map(|f| f.name).collect();
        let theirs: Vec<&str> = rm_selection::manifest::all_files()
            .iter()
            .map(|f| f.name)
            .collect();
        assert_eq!(names, theirs);
        assert_eq!(
            stored.files().iter().map(|f| f.bytes).sum::<u64>(),
            rm_selection::manifest::total_bytes()
        );
    }

    /// Only `model_manager::models_dir` names the models folder: everything
    /// else that saves a model there asks it, and fetches through
    /// `model_download` what this list holds. A feature that built the path
    /// itself would be a downloader this list cannot see.
    #[test]
    fn only_model_manager_names_the_models_folder() {
        const WRITERS: &[&str] = &["mods/model_manager.rs"];
        let needle = concat!("join(", "\"models\")");
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut found = Vec::new();
        let mut stack = vec![src.clone()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|e| e == "rs")
                    && std::fs::read_to_string(&path).unwrap().contains(needle)
                {
                    let rel = path.strip_prefix(&src).unwrap();
                    found.push(rel.to_string_lossy().replace('\\', "/"));
                }
            }
        }
        found.sort();
        let mut known: Vec<String> = WRITERS.iter().map(|s| s.to_string()).collect();
        known.sort();
        assert_eq!(found, known, "a file writes to the models folder unlisted");
    }
}
