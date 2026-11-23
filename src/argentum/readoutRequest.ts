/**
 * What the colour readout sends to Rust for one pixel (`sample_processed_pixel`).
 *
 * Rust renders that pixel with the whole edit: every slider, the AI patches
 * (they change the warped base) and the Lens blur, Relight and Fog maps. It
 * never applies masks (`mask_bitmaps: &[]`), so their bitmaps are dead weight.
 * The whole edit used to go at every mouse move, megabytes of JSON about 12
 * times a second; this is the same edit without what Rust cannot read.
 *
 * - Mask bitmaps: always dropped. The definitions stay, so the mask count Rust
 *   reads is the one the preview has.
 * - Patch data and patch bitmaps: dropped once the preview has sent them
 *   (`patchesSentToBackend`); Rust keeps them by id and puts them back
 *   (`hydrate_adjustments`).
 * - An effect map: kept only while its effect is on. Rust returns before
 *   reading it otherwise (`is_effect_enabled`). While it is on it is needed and
 *   Rust has no handle for it, so it still crosses.
 */

import type { SubMask } from '../components/panel/right/Masks';
import type { Adjustments } from '../utils/adjustments';
import { PER_PHOTO_MAPS } from './editSections';
import { withoutMaskBitmap } from './maskBitmaps';

/** The switch each map hangs on. */
export const EFFECT_OF_MAP: Record<string, string> = {
  lensBlurDepthMap: 'lensBlurEnabled',
  relightNormalMap: 'relightEnabled',
  fogDepthMap: 'fogEnabled',
};

type Loose = Record<string, unknown>;

export function readoutAdjustments(adjustments: Adjustments, sentToBackend: ReadonlySet<string>) {
  const { masks, aiPatches, ...rest } = adjustments as Adjustments & {
    aiPatches?: { id?: string; patchData?: unknown; subMasks?: SubMask[] }[];
  };
  const out: Loose = { ...rest };

  for (const map of PER_PHOTO_MAPS) {
    if (!out[EFFECT_OF_MAP[map]]) {
      delete out[map];
    }
  }

  if (masks) {
    out.masks = masks.map((mask) => ({
      ...mask,
      subMasks: mask.subMasks.map((subMask) => withoutMaskBitmap(subMask, true)),
    }));
  }
  if (aiPatches) {
    out.aiPatches = aiPatches.map((patch) => ({
      ...patch,
      patchData: patch.id && sentToBackend.has(patch.id) ? null : patch.patchData,
      subMasks: patch.subMasks?.map((subMask) => withoutMaskBitmap(subMask, sentToBackend.has(subMask.id))),
    }));
  }
  return out as Adjustments;
}
