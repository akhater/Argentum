- **Lens correction now follows a photo shot in another aspect ratio.** A Micro Four Thirds body
  that records a 16:9, 3:2 or 1:1 frame uses a smaller part of its 4:3 sensor, and Lensfun lists
  the Panasonic LX1 and LX3 once per aspect-ratio mode, each with its own crop factor. The raw's
  frame is now read when its EXIF is (rawler's default crop, without decoding the pixels): the
  Lensfun entry for its mode is chosen, and the profile is put on that frame. A fixed-lens camera
  keeps its entry's crop factor, since some have multi-aspect sensors. A 35 mm equivalent focal
  length that describes the whole sensor no longer counts as a crop mode. Photos opened before
  keep their cached EXIF and are corrected as before. The rules are RAWmakase's
  (`crates/rawmakase-model/src/lens/lensfun/matching.rs`: `camera`, `frame_crop`,
  `native_aspect`, `photo_crop`, v0.7.1 @ 26267b16, MIT). Argentum's correction is now checked
  against Lensfun's own output on RAWmakase's six reference calibrations
  (`tests/data/lensfun/reference.json`, same commit).
