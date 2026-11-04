- **Headless export with `--long-edge` is faster, and reports each photo as it is written.** The
  photo is developed at the size it is written, as the editor's preview is, instead of at full size
  and then shrunk; one GPU processor serves the whole batch, landscape or portrait; and each photo's
  JSON line is printed the moment its file is written rather than when the batch ends. On a 32 MP
  CR3 to 2048 px (Intel Arc 140V, Vulkan): one photo 3.0 s -> 1.6 s, five photos 6.5 s -> 4.0 s
  with the first line after 0.9 s. Pixels differ slightly from full size then Lanczos, because the
  shrink now happens in linear light before the tone curve: ΔE2000 mean 0.4 to 2.1, p95 1.6 to 5.7
  on five test RAWs, mostly in fine texture. The export dialog is unchanged.
