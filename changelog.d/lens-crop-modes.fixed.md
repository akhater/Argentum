- **Lens correction now follows a camera's crop mode when the file's 35 mm equivalent focal length
  tells it.** A full-frame Sony in its APS-C mode, a Nikon in DX mode, or a 1:1 readout records a
  smaller frame than the body's. Such a photo is now corrected for that frame, and a lens
  calibrated on APS-C counts as covering it. The rule is RAWmakase's (`photo_crop`,
  `crates/rawmakase-model/src/lens/lensfun/matching.rs` @ `1b2687c4`, MIT): the crop factor the 35
  mm equivalent gives, where it differs from the body's by more than rounding (10%). A camera
  Lensfun does not know takes that crop factor too, as darktable does, unless it is the lens
  profile's own within rounding. Fixed-lens cameras keep Lensfun's, since some write a 35 mm
  equivalent of the processed picture. Canon's crop mode, which only its maker note records, is
  not read yet.
