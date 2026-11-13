- **Lens distortion and vignetting were corrected too strongly on crop bodies using a lens profile
  measured on full frame.** A Lensfun profile is measured on one camera, and Argentum applied it to
  every photo as if the photo's frame were that camera's: a full-frame profile on a 1.6x body
  treated the APS-C corner as the full-frame corner, about 2.6 times too much distortion
  correction there. The profile is now put on the photo's frame as Lensfun does (darktable's
  `src/iop/lens.cc` gives Lensfun the camera's crop factor): distortion through Lensfun's own
  radius, vignetting terms scaled by the profile's crop factor over the camera's, to their power
  in the radius. A photo from a camera of the profile's own crop factor is corrected exactly as
  before. On raw.pixls.us's files, 73 of 313 automatically matched camera and lens cases change;
  of the 70 with a distortion profile, 18 come closer to Lensfun's correction, 52 move by less than
  0.1% of the radius, none moves away, and all now match Lensfun's.
