- **Exports get the right edits and names.** A virtual copy exported in the same batch as its open
  original was written with the original's live edits, and an open copy lost its unsaved ones; each
  now gets its own, in the export and in the size estimate (RapidRAW `06ffd3ca`,
  `src-tauri/src/export_processing.rs`). Virtual copies are numbered `_VC01`, `_VC02` per photo in
  batch order, instead of from the digits of the copy's id, and a file with `_vc` in its name no
  longer gets a suffix (RapidRAW `5e0a4b7a`, `src-tauri/src/export/mod.rs`). A file name template
  with folders, such as `{YYYY}/{MM}/{original_filename}`, creates them (RapidRAW `7d94545a`,
  `src-tauri/src/export_processing.rs`). The size estimate waits for the photo being opened instead
  of showing the previous one's (RapidRAW `f01c328c`). An invalid border or padding value disables
  Export instead of hiding the whole panel (RapidRAW `76ab6bb2`), Enter confirms the DPI, padding
  and border fields (RapidRAW `384f86f8`, `src/components/panel/right/ExportPanel.tsx`), and
  templates take `{ss}`, the seconds (RapidRAW `642c8f21`, `src-tauri/src/file_management.rs`).
  The new rules live in `src-tauri/src/mods/export_batch.rs`.
