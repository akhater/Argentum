import { invoke } from '@tauri-apps/api/core';
import type { Adjustments } from '../utils/adjustments';
import { stillOpen } from './stillOpen';

/**
 * Puts a lens on the photo `path` once its distortion parameters are known. The answer can come after
 * another photo was opened (stepping quickly through the Crop panel): then it is dropped, and false is
 * returned, rather than written onto the photo that is open now.
 */
export async function putLensOn(
  path: string | undefined,
  exif: Record<string, string>,
  maker: string,
  model: string,
  setAdjustments: (update: (prev: Adjustments) => Adjustments) => void,
  extra: Partial<Adjustments> = {},
): Promise<boolean> {
  let lensDistortionParams: Adjustments['lensDistortionParams'] = null;
  try {
    lensDistortionParams = await invoke<Adjustments['lensDistortionParams']>('get_lens_distortion_params', {
      maker,
      model,
      exif,
    });
  } catch (error) {
    console.error('Failed to fetch lens distortion params', error);
  }
  if (!stillOpen(path)) return false;
  setAdjustments((prev) => ({ ...prev, lensMaker: maker, lensModel: model, lensDistortionParams, ...extra }));
  return true;
}
