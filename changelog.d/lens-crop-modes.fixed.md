- **Lens correction now follows a camera's crop mode.** A full-frame Canon in its 1.6x (APS-C) or
  1.3x (APS-H) mode, a full-frame Sony in its APS-C mode, a Nikon in DX mode, or a 1:1 readout
  records a smaller frame than the body's. Such a photo is now corrected for that frame, and a lens
  calibrated on APS-C counts as covering it. Canon's crop mode is read from its maker note
  (AspectInfo); others from the 35 mm equivalent focal length where it differs from the body's
  crop by more than rounding (10%). The rule is RAWmakase's (`photo_crop`,
  `crates/rawmakase-model/src/lens/lensfun/matching.rs`, and `vendor/canon.rs::crop_mode`, @
  `1b2687c4`, MIT). A camera Lensfun does not know takes the 35 mm equivalent's crop factor, as
  darktable does, unless it is the lens profile's own within rounding. Fixed-lens cameras keep
  Lensfun's, since some write a 35 mm equivalent of the processed picture. Photos read before this
  version keep the EXIF they were read with, so a Canon crop-mode photo already in the library is
  only recognised once its metadata is read again.
