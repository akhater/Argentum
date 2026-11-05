- **Select Sky keeps the sky under leaves along the top of the frame.** When leaves, a branch or
  an eave covered the top rows, the whole column's sky was dropped. Sky starting lower is now kept
  when it starts above an estimated horizon (the lower quartile of where the sky reaching the top
  ends, or the middle of the frame), so a lake under a tree that hides the sky stays out. Skies
  that reach the top are cut as before, and saved masks are unchanged. From RAWmakase
  `crates/rawmakase-inference/src/auto.rs` @ `cf9a50dd` (MIT), in `src-tauri/rm_selection`.
