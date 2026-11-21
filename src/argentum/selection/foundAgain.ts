/**
 * Which AI masks in a preset or a paste are found again on each photo, and
 * their outlines out and back in. Ours, on RapidRAW 1.6.7's idea.
 *
 * A mask's outline is found on one photo; carried to another, a Sky mask
 * would darken the first photo's sky shape there. RapidRAW 1.6.7 takes an AI
 * mask's outline out when a preset goes on and makes it again for the photo
 * (07ef9211, a693d967: maskUtils.ts `hasAiSubMasks`, `stripAiMaskResults`,
 * `mergeAiMaskResults`). Ours does it for our selections (`agSelection`):
 * Subject, Sky, Background, Object, Text and Face, found again through our
 * own models (mods/selection_again.rs). Brushed, gradient and every other
 * mask, and their own AI masks, are carried as they are.
 *
 * Running it on photos is findAgain.ts.
 */

import type { SubMask } from '../../components/panel/right/Masks';
import type { Adjustments, MaskContainer } from '../../utils/adjustments';
import type { Ask, Found, Wanted } from '../ag.generated';

const PNG_DATA = 'data:image/png;base64,';

/** A PNG's size, read from its header (IHDR, bytes 16 to 24) without decoding it. */
export function pngSize(dataUrl: string): { width: number; height: number } | null {
  if (!dataUrl.startsWith(PNG_DATA)) return null;
  let head: string;
  try {
    head = atob(dataUrl.slice(PNG_DATA.length, PNG_DATA.length + 32));
  } catch {
    return null;
  }
  if (head.length < 24) return null;
  const at = (i: number) =>
    ((head.charCodeAt(i) << 24) |
      (head.charCodeAt(i + 1) << 16) |
      (head.charCodeAt(i + 2) << 8) |
      head.charCodeAt(i + 3)) >>>
    0;
  return { width: at(16), height: at(20) };
}

/**
 * What a sub-mask is found from, if it is one of our selections with enough
 * to find it again: Text needs its words, Face its parts, and Object its
 * clicks or box and the outline they made, whose size says where they were.
 */
export function wantedOf(subMask: Pick<SubMask, 'parameters'>): Wanted | null {
  const p = subMask.parameters ?? {};
  switch (p.agSelection) {
    case 'subject':
    case 'sky':
    case 'background':
      return { kind: p.agSelection };
    case 'text':
      return typeof p.agWords === 'string' && p.agWords.trim() ? { kind: 'text', words: p.agWords.trim() } : null;
    case 'face':
      return Array.isArray(p.agParts) && p.agParts.length > 0 ? { kind: 'face', parts: p.agParts } : null;
    case 'object': {
      const size = typeof p.maskDataBase64 === 'string' ? pngSize(p.maskDataBase64) : null;
      const clicks = Array.isArray(p.agClicks) ? p.agClicks : [];
      if (!size || (clicks.length === 0 && !p.agBounds)) return null;
      return {
        kind: 'object',
        clicks,
        bounds: p.agBounds ?? null,
        from: { ...size, orientationSteps: p.orientationSteps ?? 0 },
      };
    }
    default:
      return null;
  }
}

/** Every sub-mask in `masks` to find again on another photo. */
export const asksOf = (masks: ReadonlyArray<MaskContainer> | null | undefined): Ask[] =>
  (masks ?? []).flatMap((container) =>
    container.subMasks.flatMap((subMask) => {
      const wanted = wantedOf(subMask);
      return wanted ? [{ id: subMask.id, wanted }] : [];
    }),
  );

/**
 * `edit` with the outlines of the asked sub-masks taken out, so no photo
 * shows another's while its own is found. The same edit, untouched, when
 * nothing is asked.
 */
export function withoutOutlines<E extends Partial<Adjustments>>(edit: E, asks: ReadonlyArray<Ask>): E {
  if (asks.length === 0 || !edit.masks) return edit;
  const ids = new Set(asks.map((ask) => ask.id));
  return {
    ...edit,
    masks: edit.masks.map((container) =>
      container.subMasks.some((sm) => ids.has(sm.id))
        ? {
            ...container,
            subMasks: container.subMasks.map((sm) =>
              ids.has(sm.id) ? { ...sm, parameters: { ...sm.parameters, maskDataBase64: null } } : sm,
            ),
          }
        : container,
    ),
  };
}

/**
 * `masks` with each outline found put in its sub-mask, where that sub-mask is
 * still empty. A selection that failed found nothing to put, so its mask
 * stays empty and the rest of the edit applies.
 */
export function withOutlines(masks: ReadonlyArray<MaskContainer>, found: ReadonlyArray<Found>): MaskContainer[] {
  const outlines = new Map(found.flatMap((f) => (f.selection ? [[f.id, f.selection] as const] : [])));
  if (outlines.size === 0) return [...masks];
  return masks.map((container) => ({
    ...container,
    subMasks: container.subMasks.map((sm) => {
      const selection = outlines.get(sm.id);
      if (!selection || sm.parameters?.maskDataBase64) return sm;
      const { found: _found, ...outline } = selection;
      return { ...sm, parameters: { ...sm.parameters, ...outline } };
    }),
  }));
}
