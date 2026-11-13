- **Automatic lens correction now takes the photo's focal length into account.** A lens is no
  longer chosen for a photo taken at a focal length it does not have. Lensfun lists a lens with an
  extender or converter as its own entry, while the camera writes the bare lens's name, so these
  are now told apart: an EF 70-200mm f/2.8L IS USM at 280 mm gets the "+ EF 1.4x extender"
  calibration, and at 70 mm the lens's own. Of 58 such cases in the database, 44 are now right
  against 26 before. Photos that already matched keep their lens: on the database's own lens names
  and on raw.pixls.us's files, no answer changes. A lens's focal range is its Lensfun `<focal>`,
  else the range its name states, else the focal lengths it was calibrated at. The rule is
  RAWmakase's (`crates/rawmakase-model/src/lens/lensfun/matching.rs` and `lensfun/mod.rs` @
  `1b2687c4`, and `names.rs::stated_millimetres` @ `704ab207`, MIT).
