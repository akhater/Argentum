import { describe, expect, it } from 'vitest';
import { overlayAdjustments, overlayMaskDef } from './overlayRequest';

const MEGABYTE = 'B'.repeat(1_000_000);

type SubMaskLike = { id?: string; parameters?: Record<string, unknown> };
type OwnerLike = { subMasks?: SubMaskLike[] };

const subMask = (id: string) => ({ id, type: 'brush', parameters: { mask_data_base64: MEGABYTE, feather: 0.5 } });

/** An edit with mask bitmaps and AI patches, as the overlay request sees it. */
function heavyEdit() {
  return {
    exposure: 0.4,
    crop: { x: 1, y: 2, width: 300, height: 200 },
    fogEnabled: true,
    fogDepthMap: MEGABYTE,
    masks: [
      { id: 'm1', visible: true, adjustments: { exposure: 1 }, subMasks: [subMask('s1'), subMask('s2')] },
      { id: 'm2', visible: true, adjustments: { exposure: -1 }, subMasks: [subMask('s3')] },
    ],
    aiPatches: [
      { id: 'p1', patchData: { color: MEGABYTE }, subMasks: [subMask('s4')] },
      { id: 'p2', patchData: { color: MEGABYTE }, subMasks: [] },
    ],
  };
}

/** Blanks, in place, the bitmaps of the sent sub-masks: the code Editor.tsx ran before. */
function blankInPlace(subMasks: SubMaskLike[] | undefined, sent: Set<string>) {
  if (!Array.isArray(subMasks)) return;
  subMasks.forEach((sm) => {
    if (sm.id && sm.parameters && sent.has(sm.id)) {
      if (sm.parameters.mask_data_base64 !== undefined) sm.parameters.mask_data_base64 = null;
      if (sm.parameters.maskDataBase64 !== undefined) sm.parameters.maskDataBase64 = null;
    }
  });
}

/** The overlay request as Editor.tsx built it before: clone everything, then blank the sent bitmaps. */
function oldOverlayAdjustments<T extends { masks?: OwnerLike[]; aiPatches?: OwnerLike[] }>(
  edit: T,
  sent: Set<string>,
): T {
  const stripped = structuredClone(edit);
  stripped.masks?.forEach((m) => blankInPlace(m.subMasks, sent));
  stripped.aiPatches?.forEach((p) => blankInPlace(p.subMasks, sent));
  return stripped;
}

function oldOverlayMaskDef<T extends OwnerLike>(maskDef: T, sent: Set<string>): T {
  const stripped = structuredClone(maskDef);
  blankInPlace(stripped.subMasks, sent);
  return stripped;
}

const SENT = new Set(['s1', 's4']);

describe('the mask overlay request', () => {
  it('sends the same edit as the clone-then-blank code did', () => {
    const edit = heavyEdit();
    expect(overlayAdjustments(edit, SENT)).toStrictEqual(oldOverlayAdjustments(edit, SENT));
  });

  it('sends the same mask definition as the clone-then-blank code did', () => {
    const maskDef = { id: 'm1', visible: true, subMasks: [subMask('s1'), subMask('s3')] };
    expect(overlayMaskDef(maskDef, SENT)).toStrictEqual(oldOverlayMaskDef(maskDef, SENT));
  });

  it('does not change the edit or the mask definition it was given', () => {
    const edit = heavyEdit();
    const before = structuredClone(edit);
    const firstSubMask = edit.masks[0].subMasks[0];

    overlayAdjustments(edit, SENT);
    overlayMaskDef(edit.masks[0], SENT);

    expect(edit).toStrictEqual(before);
    expect(edit.masks[0].subMasks[0]).toBe(firstSubMask);
    expect(firstSubMask.parameters.mask_data_base64).toBe(MEGABYTE);
  });

  it('returns the edit itself when no bitmap has to go', () => {
    const edit = heavyEdit();
    expect(overlayAdjustments(edit, new Set())).toBe(edit);
  });

  it('shares every object that holds no bitmap to blank, so nothing else is copied', () => {
    const edit = heavyEdit();
    const sent = overlayAdjustments(edit, SENT);

    expect(sent.crop).toBe(edit.crop);
    expect(sent.masks[1]).toBe(edit.masks[1]);
    expect(sent.masks[0].subMasks[1]).toBe(edit.masks[0].subMasks[1]);
    expect(sent.aiPatches[0].patchData).toBe(edit.aiPatches[0].patchData);
    expect(sent.aiPatches[1]).toBe(edit.aiPatches[1]);
    expect(sent.masks[0]).not.toBe(edit.masks[0]);
  });

  it('shares the unsent sub-masks of a mask definition', () => {
    const maskDef = { id: 'm1', visible: true, subMasks: [subMask('s1'), subMask('s3')] };
    const sent = overlayMaskDef(maskDef, SENT);

    expect(sent.subMasks[0].parameters.mask_data_base64).toBeNull();
    expect(sent.subMasks[1]).toBe(maskDef.subMasks[1]);
  });
});
