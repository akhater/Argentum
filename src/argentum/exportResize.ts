/**
 * The export dialog's Resize: the size it gives the photo, and whether the
 * value typed is one Export can honour. Ours.
 *
 * The pixel limit is not written here: Rust owns it (`export_pixel_limit`,
 * from `mods/export_limits.rs`) and the panel passes it in, so the field and
 * the export refuse the same sizes.
 */

export interface SizePx {
  widthPx: number;
  heightPx: number;
}

/**
 * The exported photo's size after Resize, for the print size beside DPI.
 * Mirrors `calculate_resize_target` in src-tauri/src/export_processing.rs.
 */
export function resizedSizePx(
  widthPx: number,
  heightPx: number,
  mode: string,
  valuePx: number,
  dontEnlarge: boolean,
): SizePx {
  if (dontEnlarge) {
    const edgePx =
      mode === 'longEdge'
        ? Math.max(widthPx, heightPx)
        : mode === 'shortEdge'
          ? Math.min(widthPx, heightPx)
          : mode === 'width'
            ? widthPx
            : heightPx;
    if (edgePx <= valuePx) return { widthPx, heightPx };
  }
  const fixWidth =
    mode === 'longEdge' ? widthPx >= heightPx : mode === 'shortEdge' ? widthPx <= heightPx : mode === 'width';
  return fixWidth
    ? { widthPx: valuePx, heightPx: Math.round(valuePx * (heightPx / widthPx)) }
    : { widthPx: Math.round(valuePx * (widthPx / heightPx)), heightPx: valuePx };
}

export type ResizeCheck = 'ok' | 'invalid' | 'overLimit';

/**
 * Whether a typed Resize value can be exported. `invalid`: not a whole number
 * of at least 1 pixel. `overLimit`: the photo at `source` would have more than
 * `limit` pixels. With no `source` (several photos) or no `limit` (not asked
 * of Rust yet) only the first check applies; Rust refuses an oversized photo
 * either way.
 */
export function checkResize(
  source: SizePx | null,
  mode: string,
  value: number,
  dontEnlarge: boolean,
  limit: number | null,
): ResizeCheck {
  if (!Number.isInteger(value) || value < 1) return 'invalid';
  if (!source || limit === null) return 'ok';
  const size = resizedSizePx(source.widthPx, source.heightPx, mode, value, dontEnlarge);
  return size.widthPx * size.heightPx > limit ? 'overLimit' : 'ok';
}
