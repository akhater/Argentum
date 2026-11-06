- **Opening a folder on a drive that stopped answering no longer freezes the window.** A sleeping
  NAS, a dropped network share or a USB disk that hung could leave the whole window frozen while
  its folder was listed, because the listing ran on the window's own thread. Folders are now listed
  on a worker thread (the pattern of Shoebox's `src-tauri/src/commands.rs` @ `5a60d6f`), so the
  window keeps answering, and the folder opened last is the one shown: a slow folder that answers
  after another was opened is set aside, along with its error. Listings return exactly what they
  did before.
