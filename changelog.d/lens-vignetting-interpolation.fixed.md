- **Lens vignetting between calibrated apertures, focal lengths and distances now follows
  Lensfun.** Argentum took the profile row at the nearest aperture and distance and drew a straight
  line between focal lengths; Lensfun weights every row by its distance from the photo over focal
  length, 4/f-number and 0.1/distance, and corrects nothing when no row is near. Over the
  database's 76,443 test points the corner gain was 0.011 EV off Lensfun's typically, 0.33 EV for
  one point in twenty and up to 11.8 EV; it now matches to 0.002 EV (`libs/lensfun/lens.cpp`,
  `InterpolateVignetting` and `GuessParameters`, @ `52cf1a7b`). A photo with no subject distance
  is read at 1000 m, as darktable does. At a calibrated focal length, aperture and distance nothing changes.
