- **Noise Reduction now runs before sharpening.** Details > Noise Reduction used to smooth the
  picture after capture sharpening and Sharpen had already made its noise stronger. Its global
  sliders now run first, as in darktable, so a high-ISO photo comes out cleaner at the same
  sharpness: on an ISO 8000 RAW at 100%, Luma 60 leaves about 17% less noise than before with
  edges as steep. Its neighbourhood is also sized from the full-resolution photo, so it means the
  same at every zoom and in export. Photos with Noise Reduction at 0 render exactly as before;
  existing edits with it above 0 will look cleaner. A mask's own Noise Reduction still runs after
  sharpening, as before.
