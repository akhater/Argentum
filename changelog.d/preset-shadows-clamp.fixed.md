- **A Lightroom preset with negative Shadows now imports in range.** Lightroom's Shadows is scaled
  by 1.5 on import and was capped only at +100, so Shadows -100 came in as -150. It is now held to
  -100..100 on both ends (RapidRAW `61960fc5`, `src-tauri/src/library/preset_converter.rs`).
