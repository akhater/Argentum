# HEIC fixtures

Synthetic HEIC files for `mods/heic.rs`, copied unchanged from RAWmakase
(MIT, Copyright (c) 2026 RAWmakase contributors), `tests/data/formats/` at
v0.4.12 (`704ab207`), where `scripts/make-heic-fixtures.py` draws them with
pillow-heif. Nothing in them comes from a camera or a photo.

Each is a grey picture, 16 x 12 unless the table says otherwise, with 4 x 4
corners: red top left, green top right, blue bottom left, white bottom right.

| File | What it has |
|---|---|
| `metadata.heic` | 64 x 64 coded, cropped to 16 x 12 (`clap`); EXIF (Make `Fixture`, Model `Synthetic 1`, taken 2024:05:04 10:21:33) and XMP |
| `rotated.heic` | `irot` 1 (a quarter turn anticlockwise) |
| `rotated-mirrored.heic` | `irot` 1, then `imir` 0 |
| `p3.heic` | Display P3 primaries (`nclx` 12 / 13) |
| `grid.heic` | 120 x 100 in a 2 x 2 grid of 64 x 64 tiles, as an iPhone writes one: the ICC profile on the grid, the `nclx` on the tiles |
| `ten-bit.heic` | 10 bits per sample |
| `pq.heic` | HDR (PQ transfer), which Argentum refuses |
