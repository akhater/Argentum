- **JPEG XL export no longer runs out of memory, and keeps its metadata.** Lossless JPEG XL is
  encoded without tree learning: on five 5-32 MP RAWs the encoder's peak fell from 9.08 GB to
  2.71 GB and the time from 46 minutes to under 3, for files 13% larger. A batch now counts the
  encoder's memory before choosing how many photos to export at once (RapidRAW `892bfa30`,
  `src-tauri/src/export_processing.rs`). Keep metadata now writes EXIF and XMP (rating, label,
  keywords) into JPEG XL files, and the Export panel shows the metadata options for it (RapidRAW
  `898e4775`, `src-tauri/src/metadata/export_metadata.rs`; `9e03d499`,
  `src/components/panel/right/ExportPanel.tsx`). The rules live in
  `src-tauri/src/mods/jxl_export.rs`.
