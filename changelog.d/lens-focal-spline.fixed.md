- **Between a zoom's calibrated focal lengths, lens correction now follows Lensfun's curve.** A
  Lensfun profile is measured at a few focal lengths, and Argentum drew a straight line between the
  two nearest; Lensfun runs a smooth curve through the four nearest, on each term times its focal
  length. Between calibrations the distortion correction was up to 9% of the radius away from
  Lensfun's at the corner (0.27% for one case in twenty), and lateral CA up to 0.8 px on a 24 MP
  frame. Both now interpolate as Lensfun does (`libs/lensfun/lens.cpp`, `InterpolateDistortion`
  and `InterpolateTCA`, and `auxfun.cpp`, `_lf_interpolate`, @ `52cf1a7b`), and a profile that
  mixes distortion models uses the first, as Lensfun does. At a calibrated focal length nothing
  changes.
