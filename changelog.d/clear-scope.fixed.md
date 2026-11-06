- **Clear sidecars and Clear tags work on the open folder only, and cleared sidecars go to the
  Recycle Bin or Trash.** Since the folder column lists whole disks, these buttons in Settings
  walked every disk and permanently deleted every `.agdata` file (all edits) on them. They now
  take the folder open in the library and its subfolders, which the description shows, and are
  greyed out while no folder is open. Clear sidecars moves the files to the Recycle Bin or Trash;
  a file the bin refuses is left in place and counted, never deleted. The app also refuses a
  disk's root (`C:\`, `/`, a network share) for these clears (`mods/clear_scope.rs`).
