- **A new HDR, panorama, focus stack, collage, denoised, positive, restored or enlarged image no
  longer replaces an earlier one.** Each gets the first free name (`IMG_Hdr.tiff`, then
  `IMG_Hdr_1.tiff`, ...), and a name whose edits (`.agdata`, a virtual copy's included) or EXIF
  cache (`.agexif`) are still beside it is skipped, so a new result never opens with an old one's
  edits. The file is written whole before it gets its name, and two saves at once each keep
  theirs. From RapidRAW's `write_unique_output` (`src-tauri/src/output_files.rs` @ ea319a90,
  moved into `file_management.rs` by ec3ad01a; the save fixes of 5720d633), now
  `src-tauri/src/mods/file_ops.rs`.
