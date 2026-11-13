- **Lens distortion is now corrected by the amount and shape Lensfun gives.** Lensfun's distortion
  terms read the radius over half the short edge of the frame the lens was measured on. The
  renderer read them over the photo's half-diagonal, about 1.8 times further out on a 3:2 frame,
  with one gain of 2.5 for every term, so each power of the radius was off by a different factor.
  poly3 profiles measured on a 3:2 frame were corrected about a quarter too little. ptlens profiles,
  whose terms often have both signs, came out a different shape: an EF 24-105mm f/3.5-5.6 IS STM at
  70 mm bent the corner by 0.15% where Lensfun bends it by 1.71%. The terms are now put in the
  renderer's units as Lensfun rescales them (`libs/lensfun/mod-coord.cpp`,
  `rescale_polynomial_coefficients`, and `modifier.cpp` @ `52cf1a7b`). Of the database's 6,393
  distortion calibrations (a lens at a focal length), all but 2 now match Lensfun's correction;
  those 2 are a second calibration at a focal length the lens already has. Every photo corrected
  with a Lensfun distortion profile changes.
