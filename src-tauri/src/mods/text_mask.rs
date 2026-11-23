//! The Text mask: type what to select, such as "car" or "water", and get a
//! mask of it. From RapidRAW 1.6.7 (211ef559 text masks, ebe1d4a2 its edges
//! and detection); ours is where the answer is stored and when the models
//! load and go.
//!
//! The words become a prompt through CLIP's word list and EfficientSAM3's text
//! encoder (their `AiModels::esam3_text_prompt`), and `esam3` answers it on the
//! picture every AI mask reads (lib.rs `get_cached_full_warped_image`), with
//! their thresholds (`ESAM3_TEXT_SCORE_THRESHOLD`, `ESAM3_TEXT_REFINE_EPS`).
//! The answer is stored as the Subject mask stores its own
//! (`selection::Selection`): their Foreground mask's fields, so it renders
//! through their code unchanged.
//!
//! The four files (387 MB) are fetched only when asked, after saying their
//! size, into the models folder under the names RapidRAW 1.6.7 gives them;
//! Settings > General > AI Models lists them as "Text masks"
//! (`model_catalog::TEXT_MASKS`). One encoded picture is kept, so another word
//! on the same photo runs only the text encoder and the decoder.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
#[cfg(test)]
use std::time::Instant;

use image::{DynamicImage, GrayImage};
use ndarray::Array;
use ort::session::Session;
use ort::value::Tensor;
use tokenizers::Tokenizer;

use super::ai_session::IdleSlot;
use super::esam3::{self, PROMPT_TOKENS, Prompt};
use super::model_catalog::{TEXT_MASK_FILES, TEXT_MASKS};
use super::model_download::{self, Progress, Status};
use super::selection::{Frame, Selection, png_data_url};

/// RapidRAW's `ESAM3_TEXT_SCORE_THRESHOLD`.
const SCORE_THRESHOLD: f32 = 0.3;
/// RapidRAW's `ESAM3_TEXT_REFINE_EPS`.
const REFINE_EPS: f32 = 0.01;

const TEXT_ENCODER: &str = "efficientsam3_ev_m_text_encoder.onnx";
const TOKENIZER: &str = "clip_tokenizer.json";

/// Whether the files are there, and how much is left to fetch. By size alone:
/// cheap enough to ask before every mask.
pub fn status(dir: &Path) -> Status {
    model_download::status(dir, TEXT_MASK_FILES)
}

/// The folder last found whole (`model_download::verify`).
static VERIFIED: Mutex<Option<PathBuf>> = Mutex::new(None);

fn verify(dir: &Path) -> Result<(), String> {
    model_download::verify(dir, TEXT_MASK_FILES, &VERIFIED)
}

/// Fetch whatever is missing or wrong into the models folder `dir`, then check all four.
pub async fn download(dir: &Path, progress: impl Fn(&Progress)) -> Result<(), String> {
    model_download::download(dir, TEXT_MASKS, progress).await?;
    *VERIFIED.lock().map_err(|e| e.to_string())? = None;
    verify(dir)
}

/// What turns words into a prompt; the image encoder and decoder are `esam3`'s.
struct Loaded {
    text_encoder: Session,
    tokenizer: Tokenizer,
}

static LOADED: IdleSlot<Loaded> = IdleSlot::new("text and face mask models");

/// RapidRAW's `esam3_text_prompt`: the words as CLIP's tokens, the end token
/// kept when they run long, through the text encoder.
fn prompt(
    tokenizer: &Tokenizer,
    text_encoder: &mut Session,
    words: &str,
) -> anyhow::Result<Prompt> {
    let encoding = tokenizer
        .encode(words.trim(), true)
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    let ids = encoding.get_ids();
    let mut tokens = Array::<i64, _>::zeros((1, PROMPT_TOKENS));
    let kept = ids.len().min(PROMPT_TOKENS);
    for (i, &id) in ids[..kept].iter().enumerate() {
        tokens[[0, i]] = id as i64;
    }
    if let Some(&end) = ids.last()
        && ids.len() > PROMPT_TOKENS
    {
        tokens[[0, PROMPT_TOKENS - 1]] = end as i64;
    }

    let outputs = text_encoder.run(ort::inputs!["tokens" => Tensor::from_array(tokens)?])?;
    Ok(Prompt {
        features: outputs["language_features"]
            .try_extract_array::<f32>()?
            .to_owned(),
        padding: outputs["language_padding"]
            .try_extract_array::<f32>()?
            .to_owned(),
    })
}

/// The words as a prompt, loading the text encoder and word list first if needed.
fn words_prompt(dir: &Path, words: &str) -> Result<Prompt, String> {
    LOADED.with(
        dir,
        || {
            Ok(Loaded {
                text_encoder: esam3::load(&dir.join(TEXT_ENCODER))
                    .map_err(|e| format!("{TEXT_ENCODER}: {e}"))?,
                tokenizer: Tokenizer::from_file(dir.join(TOKENIZER)).map_err(|e| e.to_string())?,
            })
        },
        |loaded| {
            prompt(&loaded.tokenizer, &mut loaded.text_encoder, words).map_err(|e| e.to_string())
        },
    )
}

/// Find `words` in `picture` (the warped working image) with the models in
/// the models folder `dir`. `key` names the photo and its geometry.
pub fn select(
    dir: &Path,
    picture: &Arc<DynamicImage>,
    key: u64,
    words: &str,
    frame: Frame,
) -> Result<Selection, String> {
    if words.trim().is_empty() {
        return Err("Type what to select first.".to_string());
    }
    verify(dir)?;
    let prompt = words_prompt(dir, words)?;
    let found = esam3::with_picture(dir, picture, key, |encoder, decoder, features| {
        esam3::find(
            encoder,
            decoder,
            features,
            &prompt,
            picture,
            SCORE_THRESHOLD,
            REFINE_EPS,
        )
    })?;

    // Nothing found: an empty mask, as their AI masks store one (1 x 1).
    let mask = found.as_ref().filter(|m| m.pixels().any(|p| p[0] > 0));
    Ok(Selection {
        mask_data_base64: png_data_url(mask.unwrap_or(&GrayImage::new(1, 1)))?,
        rotation: frame.rotation,
        flip_horizontal: frame.flip_horizontal,
        flip_vertical: frame.flip_vertical,
        orientation_steps: frame.orientation_steps,
        found: mask.is_some(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ag-text-mask-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// An empty folder needs all four files, 387 MB; the names are the ones
    /// the loader opens.
    #[test]
    fn an_empty_folder_needs_every_file() {
        let dir = scratch("empty");
        let status = status(&dir);
        assert!(!status.installed);
        assert_eq!(status.missing_bytes, 386_690_936);
        assert_eq!(status.total_bytes, 386_690_936);
        assert!(verify(&dir).is_err());
        let names: Vec<&str> = TEXT_MASK_FILES.iter().map(|f| f.name).collect();
        assert_eq!(names[2..], [TEXT_ENCODER, TOKENIZER]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A file of the right size with the wrong bytes is not taken as there.
    #[test]
    fn the_right_size_with_the_wrong_bytes_fails_the_check() {
        let dir = scratch("wrong");
        for file in TEXT_MASK_FILES {
            std::fs::File::create(dir.join(file.name))
                .unwrap()
                .set_len(file.bytes)
                .unwrap();
        }
        assert!(status(&dir).installed);
        let err = verify(&dir).unwrap_err();
        assert!(err.contains("checksum"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The real models on real photos, to see the masks and time them: set
    /// AG_TEXT_MODELS to a folder holding the four files, AG_TEXT_PHOTOS to RAW
    /// files (';' between them), AG_TEXT_WORDS to what to look for (';'
    /// between them), AG_OUT to a folder, and ORT_DYLIB_PATH to
    /// onnxruntime.dll, then run with --ignored. Each photo's embedded preview
    /// gets `<n>-<words>.png`, the mask in red over the photo.
    #[test]
    #[ignore = "needs the models, a runtime and photos"]
    fn words_find_their_masks_on_real_photos() {
        let dir = PathBuf::from(std::env::var("AG_TEXT_MODELS").unwrap());
        let out = PathBuf::from(std::env::var("AG_OUT").unwrap());
        std::fs::create_dir_all(&out).unwrap();
        let words = std::env::var("AG_TEXT_WORDS").unwrap();
        let frame = Frame {
            rotation: 0.0,
            flip_horizontal: false,
            flip_vertical: false,
            orientation_steps: 0,
        };
        let photos = std::env::var("AG_TEXT_PHOTOS").unwrap();
        for (n, photo) in photos.split(';').filter(|p| !p.is_empty()).enumerate() {
            let bytes = std::fs::read(photo).unwrap();
            let picture =
                Arc::new(crate::raw_processing::extract_embedded_preview(&bytes).unwrap());
            for word in words.split(';').filter(|w| !w.is_empty()) {
                let started = Instant::now();
                let selection = select(&dir, &picture, n as u64, word, frame).unwrap();
                println!(
                    "{n} {word}: found {} in {:.2} s ({}x{})",
                    selection.found,
                    started.elapsed().as_secs_f32(),
                    picture.width(),
                    picture.height()
                );
                let b64 = selection.mask_data_base64.split(',').nth(1).unwrap();
                let png = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b64)
                    .unwrap();
                let mask = image::load_from_memory(&png).unwrap().to_luma8();
                let mask = image::imageops::resize(
                    &mask,
                    picture.width(),
                    picture.height(),
                    image::imageops::FilterType::Triangle,
                );
                let mut shown = picture.to_rgb8();
                for (p, m) in shown.pixels_mut().zip(mask.pixels()) {
                    let a = m[0] as f32 / 255.0 * 0.6;
                    p[0] = (p[0] as f32 * (1.0 - a) + 255.0 * a) as u8;
                    p[1] = (p[1] as f32 * (1.0 - a)) as u8;
                    p[2] = (p[2] as f32 * (1.0 - a)) as u8;
                }
                let small = image::imageops::resize(
                    &shown,
                    1200,
                    1200 * shown.height() / shown.width(),
                    image::imageops::FilterType::Triangle,
                );
                small.save(out.join(format!("{n}-{word}.png"))).unwrap();
            }
        }
    }
}
