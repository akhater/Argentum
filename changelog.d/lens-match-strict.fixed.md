- **Automatic lens correction picks the right lens more often, and never a different one where it
  was right.** A strict matcher now decides first, and only when exactly one lens fits: the camera
  is found by the make and model the file writes; a lens counts when one of its mounts is the
  camera's (or one Lensfun says fits it) and its calibration covers the camera's frame; and every
  word of the photo's lens name must be in the lens's, with the same generation, focal lengths and
  aperture, and no other maker's name. Anywhere it is not sure, the matcher used until now answers
  as before. On the 899 camera and lens pairs of raw.pixls.us's files, 876 answer as before and the
  23 that change are corrected (an "EF-S18-55mm f/3.5-5.6 III" no longer taken for the "IS II", an
  XF16-55mm no longer for its "II", an RF 70-200mm or a Tamron 17-70mm found where none was).
  On the database's own lens names, 2,229 of 2,252 are found against 2,167 before, and none found
  before is lost. Mount names in every language are read too (`slr-pentax.xml`). The matcher is
  RAWmakase's (`crates/rawmakase-model/src/lens/lensfun/matching.rs` and `names.rs` @ `704ab207`,
  MIT), in `mods/lens_match.rs` and `mods/lens_names.rs`.
