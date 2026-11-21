- **Lightroom 3 and 4 presets keep their tone settings on import.** A preset from before process
  version 2012 (2003 or 2010) lost Exposure, Contrast, Brightness, Fill Light, Recovery, Shadows,
  Clarity and its tone curve, because only the 2012 controls and curve were read. They now
  convert into the 2012 controls the way Lightroom 5's "Update to Current Process (2012)" writes
  them, the 2012 keys Lightroom 5 stores at 0 in these presets are ignored, and the preset's own
  tone curve is used. Presets from process version 2012 on import exactly as before. From
  RAWmakase (MIT): `crates/rawmakase-interop/src/lr_develop.rs` (`legacy_tone`, `eb8a3356`;
  `72eec1bf`) and `crates/rawmakase-interop/src/xmp/mod.rs` (`point_curve_keys`, `c83741ea`),
  in `src-tauri/src/mods/lr_legacy.rs`.
