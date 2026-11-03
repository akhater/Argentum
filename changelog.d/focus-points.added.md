- **Focus points: see where the camera focused.** A new icon in the photo view's top bar, after RGB
  (or Shift+F), draws the camera's autofocus points over the photo: red where the camera confirmed
  focus, white where a point was chosen, the camera's other points faint and a zone dashed. It stays
  on from photo to photo and after a restart, follows zoom, pan, rotation, flips, straightening and
  the crop, and is hidden while Transform changes the perspective or the crop tool is open. A photo
  focused by hand, or whose file records no point, says so. Read from Canon, Nikon, Sony, Fujifilm,
  Olympus / OM System, Panasonic / Leica and Pentax / Ricoh maker notes and a phone's SubjectArea,
  by Shoebox's `focus-points` crate (`src-tauri/focus-points`, commit `580c5a6d`, MIT), with Shoebox's
  maker-note finder (`src-tauri/src/imaging.rs`), adapter (`src-tauri/src/focus.rs`) and drawing
  (`src/components/FocusPoints.tsx`, `src/styles.css`) copied at the same commit. Formats researched
  from ExifTool by Phil Harvey; no ExifTool code.
