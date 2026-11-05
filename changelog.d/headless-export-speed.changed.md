- **Headless export with `--long-edge` is faster, looks the same, and reports each photo as it is
  written.** The photo is developed at twice the size it is written instead of at full size, then
  shrunk with the same Lanczos3 filter as before, now on every core; one GPU processor serves the
  whole batch, landscape or portrait; and each photo's JSON line is printed the moment its file is
  written rather than when the batch ends. Against the full-size export, ΔE2000 per pixel mean 0.73
  and p95 2.13 at worst on five test RAWs, and photos under twice the written size come out
  identical. The export dialog is unchanged.
- **Headless export picks its own GPU backend: Vulkan, then DirectX 12, then OpenGL,** whatever the
  editor's processing backend setting says (the editor itself is unchanged). On DirectX 12 every
  launch spent about 15 s compiling shaders before the first photo. Five 2048 px exports on an Intel
  Arc 140V, saved setting DirectX 12: 21.0 s -> 4.9 s; saved setting OpenGL: 8.1 s -> 5.0 s; the
  first photo arrives after about 0.9 s.
