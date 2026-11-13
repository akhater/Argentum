- **Lateral chromatic aberration is now corrected with every term of the Lensfun profile.** Most
  Lensfun profiles describe red's and blue's offset from green as a curve over the radius
  (`v + c·r + b·r²`). Argentum read only the constant `v`, so the corners kept colour fringing the
  profile corrects: up to 13 pixels on a 24 MP frame, and over 1.8 px for one calibration in
  twenty. The `c` and `b` terms are now read and applied as Lensfun applies them
  (`libs/lensfun/mod-subpix.cpp`, `ModifyCoord_TCA_Poly3` @ `52cf1a7b`), and so are the five
  profiles in Lensfun's older linear form, which were not read at all. Of the database's 3,707
  lateral CA calibrations, all now match Lensfun's at the corner.
