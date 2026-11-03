- **Photos edited before 26.44.1 now use the same camera colour as new ones.** 26.44.1 built a
  RAW's camera colour the DNG way only for new edits and kept older edits on the previous colour;
  every RAW now takes it, so an older edit's colours can shift slightly. Measured on five test
  RAWs, older edit before and after, CIEDE2000 mean 0.02 to 0.27 (p95 up to 0.62) on three
  R6 Mark III photos and 0.84 to 1.31 (p95 up to 3.47) on two 5D Mark II photos. A chosen camera
  profile still replaces the colour as before. The hidden `agDngColour` mark that new edits
  carried is no longer read and is dropped the next time an edit is saved, and a photo reset to
  its starting values still has its sidecar removed whether or not its edit carried the mark.
  Thumbnails are redrawn once (`mods/dng_colour.rs`, `mods/profile_correction.rs`).
