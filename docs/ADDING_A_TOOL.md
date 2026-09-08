# Adding a tool

Every darktable tool comes over the same way. Four files, same four every time.
Once you've seen it once, the rest are repetition.

Worked example below: **white balance**.

---

## 1. The math → `src-tauri/src/shaders/modules.wgsl` (ours)

Find darktable's version. GPU code lives in `data/kernels/*.cl`; if there's no
kernel for it, the math is in `src/iop/<name>.c`.

Translate it to WGSL and add it to our file. Wrap it in the color conversion:

```wgsl
// from darktable src/iop/<name>.c @ <commit>
fn dt_something(color: vec3<f32>, ...) -> vec3<f32> {
    let xyz = AG_SRGB_TO_XYZ * color;    // in
    // darktable's math here
    return AG_XYZ_TO_SRGB * xyz;         // out
}
```

The matrices already exist in `modules.wgsl`: sRGB ↔ XYZ, and XYZ ↔ Bradford
cone space (`AG_XYZ_TO_LMS` / `AG_LMS_TO_XYZ`) for anything that adapts between
illuminants. If a module needs a space we don't have yet, add the matrix pair
there — don't convert inline.

OpenCL → WGSL is mostly mechanical:

| OpenCL | WGSL |
|---|---|
| `float4` | `vec4<f32>` |
| `(float3)(a,b,c)` | `vec3<f32>(a,b,c)` |
| `native_powr` / `powr` | `pow` |
| `fmax` / `fmin` | `max` / `min` |
| `clamp`, `mix`, `dot` | same |

## 2. Turn it on → `shader.wgsl` (theirs — ONE line)

In `main()`, find the existing call and swap it:

```wgsl
// was:  processed_rgb = apply_white_balance(processed_rgb, t_temperature, t_tint);
processed_rgb = dt_white_balance(processed_rgb, t_temperature, t_tint);
```

Leave their old function in the file. Don't delete it — deleting causes
conflicts, and it costs nothing to leave.

**For a tool that has no RapidRAW equivalent**, add one new call line at the
right point in the chain instead.

## 3. The settings → `src-tauri/src/image_processing.rs` (theirs)

Only if the tool needs settings the app doesn't already have. White balance
reuses the existing `temperature` and `tint`, so nothing to do here.

If it needs new ones, add the field to the adjustment struct and to the
matching WGSL struct. **The two must match exactly** — same fields, same order,
or the GPU reads garbage.

## 4. The slider → `src/components/adjustments/*.tsx` (theirs)

Again, only if there's no existing control. Copy the shape of a neighbouring
slider:

```tsx
<Slider
  label={t('adjustments.color.temperature')}
  min={-100} max={100} step={1}
  value={adjustments.temperature || 0}
  onChange={(e) => handleAdjustmentChange(ColorAdjustment.Temperature, e.target.value)}
/>
```

Add the label text to `src/i18n/locales/en.json`.

---

## Check it

```bash
npm run tauri dev
```

Open a RAW, drag the slider. If the picture moves and doesn't go neon green,
the wiring is right.

Then judge it properly: same photo, same setting, side by side against
darktable. It won't match to the pixel — different pipelines — but the
character should be recognisably darktable's.

---

## Rules

- **Never restructure a file of theirs.** Add lines, don't move theirs around.
- **Never delete their function** when you replace it. Just stop calling it.
- **One tool per commit.** Makes it obvious what to undo when something looks wrong.
- **Note where you got it.** A comment with the darktable file and commit hash,
  so a year from now you can diff against upstream:

  ```wgsl
  // from darktable data/kernels/filmic.cl @ a1b2c3d
  ```

---

## Roughly how long

| | |
|---|---|
| First tool | a few days — includes building the color conversion |
| After that | 1–2 days each |
| Tools needing multiple passes (blur, denoise) | ~a week |
