# Credits

Argentum is other people's work with additions on top.

The app shows the same list under *Settings → About → Credits*, from
`src/argentum/credits.ts`, in the same groups. This file is the whole picture in
one place, because a reader on GitHub should not have to install the app to find
out whose work this is.

**The rule:** a project is credited when something of theirs is actually
shipped — not when we plan to take it, and not when we merely admired it. When
what we took changes, the wording changes with it.

---

## Built on

### [RapidRAW](https://github.com/CyberTimon/RapidRAW) — Timon Käch

Argentum is a fork of RapidRAW, taken at 1.6.6. The interface, the GPU render
pipeline, the catalogue, masking, inpainting, export, presets, tethering and
very nearly all of the application are his. Later releases are not merged;
what Argentum takes from them is credited in the CHANGELOG with its commit.

To be blunt about the proportions: almost everything you touch in Argentum is
Timon's work. Argentum changed what happens to the colour.

Licensed AGPL-3.0, which Argentum inherits unchanged.

### [darktable](https://github.com/darktable-org/darktable)

The source of Argentum's automatic white balance, and the reference it is measured
against. What has actually been taken:

| What | darktable source | Shipping? |
|---|---|---|
| Automatic white balance | `src/iop/channelmixerrgb.c`, `_auto_detect_WB()` @ `98a9ade9` | Yes |
| The unsharp mask in Sharpening | `src/iop/sharpen.c` @ `39df8424` (`mods/sharpen.rs`) | Yes |
| Raw-level behaviour, such as highlight recovery before demosaic | method from darktable's highlight reconstruction, noted where it applies in `mods/highlights.rs` and the CHANGELOG | Yes |

Beyond those, darktable's *behaviour* is the yardstick — the comparison
harness in `mods/colour_compare.rs` measures Argentum against darktable renders
of the same files. Method that came from reading darktable, rather than copied
code, is noted in [CHANGELOG.md](CHANGELOG.md) where it applies.

Licensed GPL-3.0-or-later.

---

## Taken from pull requests

Work by RapidRAW's contributors that Argentum took from their pull requests.
Each is marked in the source between `// upstream #NNNN` and
`// end upstream #NNNN`.

| Pull request | What it gave Argentum |
|---|---|
| [#1466](https://github.com/CyberTimon/RapidRAW/pull/1466), high-precision export, by dimafa | The idea behind the 16-bit TIFF export: build the export pipeline by rewriting the storage format in the shader source, and switch the 8-bit dither off with a pipeline constant. The code was written here; the 32-bit target and the tests are Argentum's |
| [#1307](https://github.com/CyberTimon/RapidRAW/pull/1307), the AI patch cache key | Two AI patches of the same length no longer share a cache entry (`cache_utils.rs`) |
| [#1633](https://github.com/CyberTimon/RapidRAW/pull/1633), the sRGB exponent | sRGB decoding uses the standard's exponent of 2.4 (`raw_processing.rs`) |

---

## Sharpening

### [RawTherapee](https://github.com/RawTherapee/RawTherapee)

Capture sharpening is RawTherapee's, by Ingo Weyrich: the Richardson-Lucy
deconvolution, the radius it reads from the RAW, and the contrast mask that keeps
it off flat areas and noise, with its automatic threshold. Ported to the GPU from
`rtengine/capturesharpening.cc` and `rt_algo.cc` (`mods/sharpen.rs`,
`mods/sharpen_gpu.rs`). The manual Sharpen beside it is darktable's (above).

---

## Camera profiles

### [RawTherapee](https://github.com/RawTherapee/RawTherapee)

Argentum reads DCP camera profiles but **ships none**. The free collections are
published with their authors' individual permission rather than under a licence
that lets anyone else redistribute them — so when Argentum fetches a profile for
your camera, it comes from RawTherapee's collection, from the project that
published it, not from us.

---

## RAW decoding

### [FlashRaw](https://github.com/ybouane/flashraw)

Settings > Processing > RAW decoder can hand the unpacking of a RAW's pixels to
FlashRaw (`flashraw` 0.1.1 on crates.io, MIT or Apache-2.0), used as a library.
rawler still reads everything else in the file, and FlashRaw is used only on the
formats where its pixels were measured identical to rawler's: Canon CR3 and CR2,
Fujifilm compressed RAF, Olympus ORF, Panasonic RW2 (v8) and Pentax PEF
(`src-tauri/engine/src/flashraw.rs`).

---

## RAW rendering

DNG Standard, under Edit > Profile > Rendering, is the default picture of a DNG renderer:
the camera's exposure offset and Adobe's default tone curve, applied as Adobe
applies it. Three sources:

### [RAWmakase](https://github.com/pch/rawmakase)

DNG Standard - the look every RAW now starts in - follows RAWmakase's default
rendering, its *Standard* profile with Lightroom's default sharpening:

- **The exposure offset for each camera body** is RAWmakase's camera table
  (`data/cameras.toml`), measured by its contributors against Camera Raw - 280
  bodies across 10 makes, kept in Argentum as `src-tauri/src/mods/dng_baseline.tsv`
  and regenerated from each RAWmakase release, which the table names.
- **The approach** - camera matrix, exposure offset, then Adobe's default tone
  curve applied so hues hold - is the one RAWmakase's Standard profile takes.
- **The default sharpening** (Sharpen 25 at radius 1) was matched against
  RAWmakase 0.3.3's render of Camera Raw's defaults on 200% crops.

**The presets that come with Argentum** are RAWmakase's (`assets/presets` @
`7c3c81a`, and `BandW` @ `a8c5d6d7`, v0.4.0; MIT): Color, Creative, B&W, Curve,
Grain and Vignetting, 26 Lightroom XMP files shipped unchanged in
`src-tauri/resources/presets/rawmakase` with their licence, and read through
Argentum's own XMP import. The Amount slider for a preset follows its rule: it
starts from the photo as it was and moves only what the preset changes.

**B&W's black and white mix** is RAWmakase's (`black_white.rs` @ `a8c5d6d7`, its
chart at v0.4.1, unchanged since): the grey Camera Raw 18.7 makes of each colour, measured by its
contributors on a colour chart at a zero mix and with each of the eight bands at
four positions, kept in Argentum as `src-tauri/engine/src/rm_black_white_chart.tsv`
and regenerated from each RAWmakase release, which the chart names.

**Color Grading**, for new edits and Looks, is RAWmakase's
(`color_grade_curves.rs` and its tables @ `5a2414b4`, v0.4.1, unchanged since
v0.4.0): Camera Raw 18.7's grading, measured by its contributors on a colour
chart under about 740 grading settings and fitted as a curve per channel of
linear ProPhoto RGB, kept in Argentum as
`src-tauri/engine/src/rm_color_grade_curves.tsv` and regenerated from each
RAWmakase release, which the table names.

**The camera's own lens correction** (Crop > Lens Correction) is read as
RAWmakase reads it: the distortion, lateral chromatic aberration and vignetting
tables each camera make stores in its RAW files, their units, and which of them
Lightroom applies by default (`crates/rawmakase-model/src/lens/embedded.rs`,
`optics/mod.rs` and `dng.rs`, from v0.4.0), and, from v0.4.6 (`52da1bf`), the optical
centre each is measured from and Lightroom's framing of the corrected photo. Each make's reader in
`src-tauri/engine/src/rm_lens_builtin/` names the RAWmakase commit it was copied from.
RAWmakase's own record of what its decoder reads from the golden photos
(`tests/corpus/dataset/pixls-decode.json`) is the test the copies are held to
(`src-tauri/tests/look/rm_lens_baselines.tsv`).

**Some RAW files are read as RAWmakase measured them**, in Argentum's copy of
rawler (`akhater/RapidRAW-DngLab`): the Canon EOS R8 Mark II's colour data
(ColorData version 67, `canon_cr3.rs` @ `3c54413`), and the corrected colour
matrices and white levels of a few bodies (`data/raw-matrices.toml` and
`data/raw-cameras.toml` @ `a8c5d6d7`), which carry Adobe DNG Converter 18.7's.
Noise below black is clipped where RAWmakase clips it, after demosaicing a Bayer
sensor's samples (`crates/rawmakase-decode/src/demosaic.rs` @ `4d3f6f8c`).

**The names Adobe profiles some cameras under** - the Lumix G97 and its twins as
the DC-G99, the E-M10 Mark III S as the Mark III, and a few more - are RAWmakase's
(`data/adobe-camera-names.toml`), each checked by its contributors against what
Camera Raw installs, kept in Argentum as `src-tauri/src/mods/adobe_camera_names.tsv`
and regenerated from each RAWmakase release, which the table names.

RAWmakase is also where the comparison started: it showed that the flatness of
Argentum's default rendering was the tone, not the decoder. What Argentum does not
take from it: its own decoder and demosaic, its *Color* look, its measured
highlight and shadow tables, and its default colour noise reduction. MIT licence, in
full in [docs/licenses/RAWmakase-MIT.txt](docs/licenses/RAWmakase-MIT.txt).

### [RawTherapee](https://github.com/RawTherapee/RawTherapee)

How the curve is applied - to each pixel's largest and smallest channel, the
middle one kept in its place between them - is RawTherapee's
`AdobeToneCurve::RGBTone` (`rtengine/curves.h` @ `46b92d8e`), ported to the GPU as
`rt_rgb_tone`. The curve's 1025 samples are copied from RawTherapee's
`rtengine/dcp.cc` @ `ca9234b1`.

### Adobe DNG SDK

The default tone curve itself is Adobe's, from the DNG SDK, copyright Adobe Inc.,
distributed under the SDK's licence:
[docs/licenses/Adobe-DNG-SDK.txt](docs/licenses/Adobe-DNG-SDK.txt). Argentum is not
affiliated with or endorsed by Adobe; Adobe, Lightroom and Camera Raw are
trademarks of Adobe Inc.

---

## AI masks

### [RAWmakase](https://github.com/pch/rawmakase)

Subject, Sky and Background, and Object by clicking, are RAWmakase's selection
code (`crates/rawmakase-inference` @ `7c3c81a`), taken whole into
`src-tauri/rm_selection`: DETR finds the people, animals and sky, SAM 2.1 draws
their outlines, and its edge refinement and its rule for where a sky stops are
kept as written. MIT.

### [SAM 2.1](https://github.com/facebookresearch/sam2) — Meta AI

Segment Anything Model 2.1, Hiera small, draws the outline of each selection.
Apache-2.0; ONNX export by the Hugging Face onnx-community.

### [DETR](https://github.com/facebookresearch/detr) — Facebook AI Research

DETR ResNet-50 panoptic, trained on COCO, says where a photo's people, animals and
sky are. Apache-2.0; ONNX export by Xenova.

### [EfficientSAM3](https://github.com/SimonZeng7108/efficientsam3)

The Text mask finds what is typed with EfficientSAM3 EV-M, an image encoder, a
text encoder and a decoder distilled from Meta's SAM 3, reading the words through
CLIP's word list. Apache-2.0 as its authors publish it; SAM 3 itself is under
Meta's SAM License. The ONNX files and the code that runs them are RapidRAW
1.6.7's (`ai_processing.rs` @ `211ef559` and `ebe1d4a2`). It also finds the faces
for the Face mask.

### [MediaPipe](https://github.com/google-ai-edge/mediapipe) — Google

The Face mask draws each face's parts with MediaPipe's face landmarks (478 points)
and its selfie multiclass segmenter (hair, face and body skin). Apache-2.0. The
ONNX files and the code that runs them are RapidRAW 1.6.7's (`face_masking.rs` @
`07ef9211` and `49beb431`).

---

## AI enlargement and restoration

### [Real-ESRGAN](https://github.com/xinntao/Real-ESRGAN)

The local 2x and 4x photo enlargement models. Argentum downloads the selected
ONNX artifact on first use, verifies its SHA-256 digest, and runs it locally
through ONNX Runtime. The original project is released under the BSD-3-Clause
licence.

### [OmniLens](https://github.com/zju-jiangqi/OmniLens)

Restore's model, by Qi Jiang and colleagues at Zhejiang University: a SwinIR
network (Jingyun Liang et al.) pretrained on a library of thousands of
simulated lens designs, so it undoes the softness of a lens it has never seen.
Argentum converts it to ONNX, publishes that copy in
[argentum-models](https://github.com/akhater/argentum-models), and downloads it
the first time Restore is used. Code and weights are Apache-2.0.

### [ONNX Runtime with DirectML](https://github.com/microsoft/onnxruntime)

Microsoft's DirectML build of the runtime RapidRAW already uses. On Windows it is
downloaded the first time a photo is enlarged or restored, and runs the model on
the graphics card. MIT licence.

---

## Inherited with RapidRAW

Argentum ships all of RapidRAW's functionality, so everything he depends on,
Argentum depends on too. These are his credits, repeated here because they are
just as true of this app.

| Project | What it does here |
|---|---|
| [rawler](https://github.com/dnglab/dnglab) | RAW decoding. The foundation everything else stands on |
| [lensfun](https://lensfun.github.io/) | The lens correction library and its camera/lens database |
| [Segment Anything (SAM)](https://github.com/facebookresearch/segment-anything) | The model that cuts out what a box is drawn around, for Quick Erase and Inpainting |
| [U²-Net](https://github.com/xuebinqin/U-2-Net) | The architecture behind Foreground detection |
| [LaMa](https://github.com/advimman/lama) | Inpainting — content-aware fill and object removal |
| Depth Anything | Depth estimation, which drives the Depth mask, Lens blur and Fog |
| NIND | The models behind AI noise reduction |
| [NegPy](https://github.com/marcinz606/NegPy) | The approach the film-negative conversion is based on |
| Spektrafilm — Andrea Volpato | The spectrally-based film emulation LUTs. Licensed CC BY-SA 4.0 |
| [libgphoto2](https://github.com/gphoto/libgphoto2) | Camera communication for tethering, only in builds from source for macOS and Linux (`--features tethering`) |

Two entries are deliberately named without a link: **Depth Anything** and
**NIND** have several plausible upstreams and none is cited in the code, so
naming them is honest where guessing a URL would not be.

---

## Licence

Argentum is licensed **AGPL-3.0** — RapidRAW's licence, inherited unchanged. See
[LICENSE](LICENSE).

darktable is GPL-3.0-or-later, which is compatible with the AGPL: the combined
work is distributed under the AGPL. Spektrafilm's LUTs are CC BY-SA 4.0 and are
attributed above as that licence requires.
