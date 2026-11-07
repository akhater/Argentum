- **HEIC and HEIF photos open.** iPhone photos (`.heic`, `.heif`, `.hif`) are listed, get
  thumbnails, open in the editor and export like a JPEG, upright and with their EXIF. Their
  Display P3 colour is converted to sRGB rather than shown as sRGB. Argentum ships no HEVC
  decoder: it uses the system's own, so on Windows they need Microsoft's HEIF Image Extensions
  and HEVC Video Extensions from the Microsoft Store, and on Linux libheif with its libde265
  plugin; without them a HEIC photo is still listed, and opening it says what to install. HDR
  HEIF photos are not opened yet; only the primary image of a file is shown (no depth map, Live
  Photo video or burst). Adapted from RAWmakase v0.4.12 (`704ab207`):
  `crates/rawmakase-decode/src/raster/heic.rs`, `raster/heif.rs`, `raster/heic/wic.rs`,
  `raster/heic/imageio.rs`, `raster/heic/libheif.rs` and `crates/rawmakase-interop/src/isobmff.rs`.
