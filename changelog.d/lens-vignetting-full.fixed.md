- **Lens vignetting is now corrected by the whole of the Lensfun profile.** Lensfun's vignetting
  profiles are flat-field measurements, and Argentum applied only 0.8 of their correction at the
  Vignetting slider's 100%, a factor inherited without a stated reason beside the distortion gain
  that the renderer's units fix replaced. The corners were left up to a third of a stop darker
  than the profile measures (0.08 EV for half of the database's calibrations). The slider's 100% is
  now Lensfun's full correction, and 0% none; photos corrected with a Lensfun profile get brighter
  corners.
