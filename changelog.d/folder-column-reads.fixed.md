- **The folder column reads a folder when you open it.** It used to look inside every folder it
  listed, and at start one level below each folder left unfolded, for the arrows and photo counts.
  On a Mac that raised Desktop, Apple Music and Photo Library permission prompts with nothing
  opened. Now a folded disk is read only when it is opened or the column is searched, on every
  system, and folders in a Mac home folder, online-only folders and mounted disks show an arrow and
  no count until they are opened. Photos, Music and TV libraries and apps are no longer listed as
  folders on a Mac, and a recursive listing skips them. Arrows and counts elsewhere are unchanged. The rule is `src-tauri/src/mods/folder_walk.rs`; RapidRAW's
  `has_subdirs` it narrows is at 79c2a46b.
