- **JPEG exports from photos with more than about 64 KB of EXIF are no longer corrupt.** A JPEG's
  EXIF must fit one segment, and little_exif wrote a longer one with a length that wraps, so
  readers lost the rest of the file while the export reported success. The EXIF is now trimmed to
  fit before it is written, dropping the largest values first, the EXIF directory's before the main
  one's, and keeping the camera, lens, capture time, exposure, artist, copyright and GPS
  (`mods/jpeg_exif_fit.rs`). The rule is RAWmakase's
  (`crates/rawmakase-export/src/export/metadata.rs` @ `23504b93`, MIT).
