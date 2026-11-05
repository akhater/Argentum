- **Headless export with `--long-edge` is faster, looks the same, and reports each photo as it is
  written.** The photo is developed at twice the size it is written instead of at full size, then
  shrunk with the same Lanczos3 filter as before, now on every core; one GPU processor serves the
  whole batch, landscape or portrait; and each photo's JSON line is printed the moment its file is
  written rather than when the batch ends. On a 32 MP CR3 to 2048 px (Intel Arc 140V, Vulkan): one
  photo 4.4 s -> 2.4 s, five photos 13.6 s -> 7.0 s with the first line after 1.3 s instead of 13 s.
  Against the full-size export, ΔE2000 per pixel mean 0.73 and p95 2.13 at worst on five test RAWs,
  and photos under twice the written size come out identical. The export dialog is unchanged.
- **DirectX 12 builds its shaders with DXC when the app carries `dxcompiler.dll` in its resources**,
  instead of FXC: building the GPU processor took 15 s with FXC and 1.7 s with DXC, so a headless
  export of one photo on DirectX 12 went from 21 s to 3.2 s.
