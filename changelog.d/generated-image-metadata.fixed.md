- **Merged and converted images keep the camera's EXIF, and TIFFs carry the sRGB profile.** An
  HDR, panorama, focus stack, denoised, positive, restored or enlarged image now has its source
  photo's camera, lens, date and GPS inside the file, as an export of that photo does, so other
  apps sort and read it right (before, only Argentum's own edits file held them). HDR and panorama
  TIFFs stay 32-bit float. Every TIFF export and generated TIFF is tagged with the sRGB ICC
  profile JPEG and PNG exports already carry, so colour-managed apps show it as intended. From
  RapidRAW's `save_converted_image` (`src-tauri/src/exif_processing.rs` @ 36bb789a) and its TIFF
  profile tag (`src-tauri/src/tiff_metadata.rs` @ 31d77012), now `src-tauri/src/mods/generated_image.rs`
  and `encode_tiff` in `src-tauri/src/mods/export_precision.rs`.
