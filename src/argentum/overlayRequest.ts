/**
 * What the mask overlay sends to Rust (`generate_mask_overlay`): the mask
 * definition and the edit, with the bitmaps the preview has already sent blanked.
 *
 * The edit can hold megabytes of base64. Copying all of it to blank a few
 * strings was the cost, so this copies only the objects on the path to each
 * blanked bitmap and shares everything else with the edit it was given.
 *
 * A sub-mask's bitmap is blanked only when its id is in `sentToBackend`, as the
 * preview does for patches Rust already holds.
 */

import { withoutMaskBitmap } from './maskBitmaps';

type SubMaskLike = { id?: string; parameters?: Record<string, unknown> };
type OwnerLike = { subMasks?: SubMaskLike[] };

/** Maps the items; returns the same array when no item changed. */
function mapChanged<T>(items: T[], change: (item: T) => T): T[] {
  let changed = false;
  const next = items.map((item) => {
    const result = change(item);
    if (result !== item) changed = true;
    return result;
  });
  return changed ? next : items;
}

/** A mask or AI patch with its sub-masks blanked, or the same object if none changed. */
function blankOwner<T extends OwnerLike>(owner: T, sentToBackend: ReadonlySet<string>): T {
  if (!Array.isArray(owner.subMasks)) return owner;
  const subMasks = mapChanged(owner.subMasks, (subMask) =>
    withoutMaskBitmap(subMask, !!subMask.id && sentToBackend.has(subMask.id)),
  );
  return subMasks === owner.subMasks ? owner : { ...owner, subMasks };
}

/** The edit with the bitmaps of sent sub-masks blanked. The edit is not changed. */
export function overlayAdjustments<T extends object>(adjustments: T, sentToBackend: ReadonlySet<string>): T {
  const { masks, aiPatches } = adjustments as T & { masks?: unknown; aiPatches?: unknown };
  let out = adjustments;
  if (Array.isArray(masks)) {
    const nextMasks = mapChanged(masks as OwnerLike[], (mask) => blankOwner(mask, sentToBackend));
    if (nextMasks !== masks) out = { ...out, masks: nextMasks };
  }
  if (Array.isArray(aiPatches)) {
    const nextPatches = mapChanged(aiPatches as OwnerLike[], (patch) => blankOwner(patch, sentToBackend));
    if (nextPatches !== aiPatches) out = { ...out, aiPatches: nextPatches };
  }
  return out;
}

/** The mask definition with the bitmaps of sent sub-masks blanked. The definition is not changed. */
export function overlayMaskDef<T extends OwnerLike>(maskDef: T, sentToBackend: ReadonlySet<string>): T {
  return blankOwner(maskDef, sentToBackend);
}
