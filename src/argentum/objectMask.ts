/**
 * The object brush: the logic, with no UI. Ours.
 *
 * Paint roughly over something and the mask snaps to it — Lightroom's Select
 * Object. The canvas side is `ObjectBrush.tsx`; the segmentation is
 * `src-tauri/src/mods/object_brush.rs`.
 *
 * HOW IT GETS INTO THEIR TOOLBOX WITHOUT TOUCHING IT
 *
 * RapidRAW's masks panel draws its tool grid from `MASK_AI_TYPES`, an array it
 * exports and reads on every render, and labels each tile by its type string.
 * So the tool is one more entry in that array, added from here at startup, with
 * the type `object` — whose label their fallback capitalises to "Object".
 *
 * A mask of type `object` is something nothing of theirs can draw, so it does
 * not stay one. The moment their panel creates it, `adoptObjectMasks` turns it
 * into an ordinary `ai-subject` mask carrying `objectStrokes`. From then on
 * their renderer, their export, their grow and feather sliders, and their
 * embedding warm-up all treat it as a Subject mask, which it is. The strokes are
 * what marks it as ours, and what the canvas listens for.
 *
 * Every piece of theirs this leans on is listed in the `object-brush` entry of
 * scripts/upstream-registry.mjs.
 */

import { Paintbrush } from 'lucide-react';
import { ag } from './ag';
import { useEditorStore } from '../store/useEditorStore';
import { Adjustments, MaskContainer } from '../utils/adjustments';
import { ALL_MASK_TYPES, MASK_AI_TYPES, MASK_ICON_MAP, Mask, MaskType, SubMask } from '../components/panel/right/Masks';

/** The type the toolbox tile creates, for as long as it takes to adopt it. */
export const OBJECT_TYPE = 'object' as Mask;

export interface ObjectStroke {
  /** Their mask coordinates: the rotated photo, before crop. */
  points: { x: number; y: number }[];
  /** Brush radius, in the same pixels. */
  radius: number;
  /** Painted with Alt: leave this out. */
  exclude?: boolean;
}

/**
 * Put the tile in their grid, after Subject.
 *
 * Idempotent, because a hot reload runs this module again against the same
 * arrays. A throw here would happen at import time and take the whole UI down,
 * so it cannot be allowed out.
 */
export function registerObjectTool() {
  try {
    if (MASK_AI_TYPES.some((t) => t.type === OBJECT_TYPE)) {
      return;
    }
    const tile: MaskType = { disabled: false, icon: Paintbrush, name: 'Object', type: OBJECT_TYPE };
    const subject = MASK_AI_TYPES.findIndex((t) => t.type === Mask.AiSubject);
    MASK_AI_TYPES.splice(subject >= 0 ? subject + 1 : MASK_AI_TYPES.length, 0, tile);
    ALL_MASK_TYPES.push(tile);
    (MASK_ICON_MAP as Record<string, unknown>)[OBJECT_TYPE] = Paintbrush;
  } catch (e) {
    console.error('Argentum: could not add the object brush to the masks toolbox', e);
  }
}

export function isObjectMask(subMask: SubMask | null | undefined): boolean {
  return subMask?.type === Mask.AiSubject && Array.isArray(subMask.parameters?.objectStrokes);
}

/**
 * Turn any freshly created `object` masks into Subject masks that carry strokes.
 * Returns null when there is nothing to adopt, so callers can skip the write.
 */
export function adoptObjectMasks(adjustments: Adjustments): Adjustments | null {
  const masks: MaskContainer[] = adjustments.masks || [];
  if (!masks.some((c) => c.subMasks.some((sm) => sm.type === OBJECT_TYPE))) {
    return null;
  }
  return {
    ...adjustments,
    masks: masks.map((c) => ({
      ...c,
      subMasks: c.subMasks.map((sm) =>
        sm.type === OBJECT_TYPE
          ? {
              ...sm,
              type: Mask.AiSubject,
              // Their defaults for a new Subject mask, from createSubMask.
              parameters: { maskDataBase64: null, grow: 0, feather: 0, ...sm.parameters, objectStrokes: [] },
            }
          : sm,
      ),
    })),
  };
}

/** The active mask's component, if it is an object brush. */
export function findObjectMask(adjustments: Adjustments, activeMaskId: string | null): SubMask | null {
  if (!activeMaskId) {
    return null;
  }
  for (const c of adjustments.masks || []) {
    const sm = c.subMasks.find((s) => s.id === activeMaskId);
    if (sm) {
      return isObjectMask(sm) ? sm : null;
    }
  }
  return null;
}

export function activeObjectMask(): SubMask | null {
  const { adjustments, activeMaskId } = useEditorStore.getState();
  return findObjectMask(adjustments, activeMaskId);
}

/**
 * The adjustments the AI mask commands are sent. Exactly their subset, from
 * `getTransformAdjustments` in useAiMasking.ts: the embedding cache is keyed on
 * these, and their warm-up sends this subset, so sending anything else would
 * miss the cache and encode the photo a second time.
 */
function transformAdjustments(adj: Adjustments) {
  return {
    transformDistortion: adj.transformDistortion,
    transformVertical: adj.transformVertical,
    transformHorizontal: adj.transformHorizontal,
    transformRotate: adj.transformRotate,
    transformAspect: adj.transformAspect,
    transformScale: adj.transformScale,
    transformXOffset: adj.transformXOffset,
    transformYOffset: adj.transformYOffset,
    lensDistortionAmount: adj.lensDistortionAmount,
    lensVignetteAmount: adj.lensVignetteAmount,
    lensTcaAmount: adj.lensTcaAmount,
    lensDistortionParams: adj.lensDistortionParams,
    lensMaker: adj.lensMaker,
    lensModel: adj.lensModel,
    lensDistortionEnabled: adj.lensDistortionEnabled,
    lensTcaEnabled: adj.lensTcaEnabled,
    lensVignetteEnabled: adj.lensVignetteEnabled,
  };
}

/** Ask Rust for the mask these strokes describe. */
export async function segmentStrokes(strokes: ObjectStroke[]): Promise<Record<string, unknown>> {
  const { selectedImage, adjustments } = useEditorStore.getState();
  if (!selectedImage?.path) {
    throw new Error('no photo open');
  }
  return ag<Record<string, unknown>>('object_brush_mask', {
    jsAdjustments: transformAdjustments(adjustments),
    path: selectedImage.path,
    strokes,
    rotation: adjustments.rotation,
    flipHorizontal: adjustments.flipHorizontal,
    flipVertical: adjustments.flipVertical,
    orientationSteps: adjustments.orientationSteps,
  });
}

/** Where the photo's crop sits, in their mask coordinates. */
export function cropFrame(): { x: number; y: number; width: number; height: number } | null {
  const { selectedImage, adjustments } = useEditorStore.getState();
  if (!selectedImage?.width || !selectedImage?.height) {
    return null;
  }
  // As ImageCanvas computes it: the photo after quarter turns, before crop.
  const turned = (adjustments.orientationSteps || 0) % 2 === 1;
  const width = turned ? selectedImage.height : selectedImage.width;
  const height = turned ? selectedImage.width : selectedImage.height;
  const crop = adjustments.crop;
  if (!crop) {
    return { x: 0, y: 0, width, height };
  }
  const pct = crop.unit === '%';
  return {
    x: pct ? (crop.x / 100) * width : crop.x,
    y: pct ? (crop.y / 100) * height : crop.y,
    width: pct ? (crop.width / 100) * width : crop.width,
    height: pct ? (crop.height / 100) * height : crop.height,
  };
}
