# Lensfun's own corrections, for the lens-correction tests

`reference.json` is RAWmakase's `crates/rawmakase-model/tests/data/lensfun/reference.json`
at v0.7.1 (26267b16, MIT), unchanged: six synthetic Lensfun calibrations (made up for
the tests, no database data) and, for each, what Lensfun itself computes on a photo
of a given crop factor and size. RAWmakase wrote it with Lensfun at bc5615d6 through
`probe.cpp` and `make_reference.py` in the same folder:

- `d`, `t`, `dt`: the red, green and blue source coordinates of each output point
  with distortion, lateral CA, or both corrected;
- `v`: the vignetting gain at each point;
- `on`: the corrections Lensfun enabled.

`mods/lens_reference.rs` compares Argentum's correction with it.
