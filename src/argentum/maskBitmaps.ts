/** Where a sub-mask keeps its bitmap, under either spelling. */
export const MASK_BITMAP_KEYS = ['mask_data_base64', 'maskDataBase64'];

type WithParameters = { parameters?: Record<string, unknown> };

/**
 * The sub-mask with its bitmap blanked when `gone`. Copies only the sub-mask and
 * its parameters, and only when there is a bitmap to blank; otherwise returns
 * the sub-mask itself.
 */
export function withoutMaskBitmap<T extends WithParameters>(subMask: T, gone: boolean): T {
  if (!gone || !subMask.parameters) {
    return subMask;
  }
  const parameters = { ...subMask.parameters };
  for (const key of MASK_BITMAP_KEYS) {
    if (parameters[key] !== undefined && parameters[key] !== null) {
      parameters[key] = null;
    }
  }
  return { ...subMask, parameters };
}
