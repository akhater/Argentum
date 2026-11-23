import { describe, expect, it } from 'vitest';
import { PER_PHOTO_MAPS } from './editSections';
import { EFFECT_OF_MAP, readoutAdjustments } from './readoutRequest';

const MEGABYTE = 'A'.repeat(1_000_000);

const subMask = (id: string) => ({ id, type: 'brush', parameters: { maskDataBase64: MEGABYTE, feather: 0.5 } });

/** An edit as it is after masking, AI patches and every map: megabytes of JSON. */
function heavyEdit(effects: Record<string, boolean> = {}) {
  return {
    exposure: 0.4,
    temperature: -20,
    crop: { x: 1, y: 2, width: 300, height: 200 },
    lensBlurEnabled: false,
    relightEnabled: false,
    fogEnabled: false,
    ...effects,
    lensBlurDepthMap: MEGABYTE,
    relightNormalMap: MEGABYTE,
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

const bytes = (value: unknown) => JSON.stringify(value).length;

describe('the colour readout request', () => {
  it('has no mask bitmap and no effect map, and is a few KB', () => {
    const edit = heavyEdit();
    expect(bytes(edit)).toBeGreaterThan(8_000_000);

    const sent = readoutAdjustments(edit as never, new Set(['p1', 'p2', 's4']));
    const json = JSON.stringify(sent);

    expect(json).not.toContain('AAAA');
    expect(bytes(sent)).toBeLessThan(2_000);
  });

  it('keeps what Rust reads: the edit, the mask definitions, the patch ids', () => {
    const sent = readoutAdjustments(heavyEdit() as never, new Set(['p1', 'p2']));

    expect(sent.exposure).toBe(0.4);
    expect(sent.crop).toEqual({ x: 1, y: 2, width: 300, height: 200 });
    expect(sent.masks?.map((m) => [m.id, m.subMasks.map((s) => s.id)])).toEqual([
      ['m1', ['s1', 's2']],
      ['m2', ['s3']],
    ]);
    expect(sent.masks?.[0].subMasks[0].parameters).toEqual({ maskDataBase64: null, feather: 0.5 });
    expect((sent as { aiPatches: { id: string }[] }).aiPatches.map((p) => p.id)).toEqual(['p1', 'p2']);
  });

  it('sends a patch Rust has not been given yet, as the preview does', () => {
    const sent = readoutAdjustments(heavyEdit() as never, new Set(['p1'])) as unknown as {
      aiPatches: { patchData: unknown }[];
    };
    expect(sent.aiPatches[0].patchData).toBeNull();
    expect(sent.aiPatches[1].patchData).not.toBeNull();
  });

  it('keeps a map only while its effect is on, because Rust reads it only then', () => {
    const sent = readoutAdjustments(heavyEdit({ fogEnabled: true }) as never, new Set()) as unknown as Record<
      string,
      unknown
    >;
    expect(sent.fogDepthMap).toBe(MEGABYTE);
    expect('lensBlurDepthMap' in sent).toBe(false);
    expect('relightNormalMap' in sent).toBe(false);
  });

  it('does not change the edit it was given', () => {
    const edit = heavyEdit({ fogEnabled: true });
    readoutAdjustments(edit as never, new Set(['p1']));
    expect(edit.masks[0].subMasks[0].parameters.maskDataBase64).toBe(MEGABYTE);
    expect(edit.aiPatches[0].patchData).not.toBeNull();
    expect(edit.lensBlurDepthMap).toBe(MEGABYTE);
  });

  it('covers every per-photo map the editor knows', () => {
    expect(Object.keys(EFFECT_OF_MAP).sort()).toEqual([...PER_PHOTO_MAPS].sort());
  });
});
