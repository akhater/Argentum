import { describe, expect, it } from 'vitest';
import type { MaskContainer } from '../../utils/adjustments';
import type { Found, Selection } from '../ag.generated';
import { lookOnto } from '../looks/providerLook';
import { asksOf, pngSize, wantedOf, withOutlines, withoutOutlines } from './foundAgain';

/** A PNG's first 24 bytes (signature and IHDR), as a data URL: enough for its size. */
function pngOf(width: number, height: number): string {
  const bytes = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, 0x49, 0x48, 0x44, 0x52];
  for (const n of [width, height]) bytes.push((n >>> 24) & 255, (n >>> 16) & 255, (n >>> 8) & 255, n & 255);
  bytes.push(8, 0, 0, 0, 0, 0, 0, 0);
  return `data:image/png;base64,${btoa(String.fromCharCode(...bytes))}`;
}

const FIRST_PHOTO_SKY = pngOf(600, 400);

const subMask = (id: string, type: string, parameters: object) => ({
  id,
  type,
  name: id,
  visible: true,
  invert: false,
  opacity: 100,
  mode: 'additive',
  parameters,
});

const container = (id: string, subMasks: object[]) =>
  ({
    id,
    name: id,
    visible: true,
    invert: false,
    opacity: 100,
    adjustments: { exposure: -1 },
    subMasks,
  }) as unknown as MaskContainer;

const handMade = () => [
  container('brushed', [
    subMask('brush', 'brush', { lines: [{ points: [{ x: 1, y: 2 }], brushSize: 40, feather: 0.5, tool: 'brush' }] }),
    subMask('linear', 'linear', { startX: 0, startY: 0, endX: 100, endY: 100, range: 50 }),
  ]),
];

/** A preset made on one photo: its Sky's outline is that photo's sky. */
const presetMasks = (): MaskContainer[] => [
  container('sky', [
    subMask('sky1', 'ai-sky', { agSelection: 'sky', maskDataBase64: FIRST_PHOTO_SKY, grow: 0, feather: 0 }),
    subMask('grad', 'linear', { startX: 0, startY: 0, endX: 10, endY: 10, range: 5 }),
  ]),
  container('people', [
    subMask('subj', 'ai-foreground', { agSelection: 'subject', maskDataBase64: pngOf(600, 400) }),
    subMask('face', 'ai-foreground', { agSelection: 'face', agParts: ['lips'], maskDataBase64: pngOf(600, 400) }),
    subMask('car', 'ai-foreground', { agSelection: 'text', agWords: ' car ', maskDataBase64: pngOf(600, 400) }),
  ]),
  ...handMade(),
];

const outline = (data: string): Selection => ({
  maskDataBase64: data,
  rotation: 0,
  flipHorizontal: false,
  flipVertical: false,
  orientationSteps: 0,
  found: true,
});

const params = (masks: MaskContainer[], id: string) =>
  masks.flatMap((c) => c.subMasks).find((sm) => sm.id === id)?.parameters;

describe('AI masks found again on each photo', () => {
  it('today a preset carries its outlines onto another photo as they were found', () => {
    const preset = { exposure: 0.5, masks: presetMasks() };
    expect(params(lookOnto({}, preset).masks!, 'sky1').maskDataBase64).toBe(FIRST_PHOTO_SKY);
  });

  it('asks for each of our selections, with what it is found from', () => {
    expect(asksOf(presetMasks())).toEqual([
      { id: 'sky1', wanted: { kind: 'sky' } },
      { id: 'subj', wanted: { kind: 'subject' } },
      { id: 'face', wanted: { kind: 'face', parts: ['lips'] } },
      { id: 'car', wanted: { kind: 'text', words: 'car' } },
    ]);
  });

  it('a preset with an AI mask is resolved per photo', () => {
    const preset = { exposure: 0.5, masks: presetMasks() };
    const asks = asksOf(preset.masks);
    const stripped = withoutOutlines(preset, asks);
    expect(params(stripped.masks, 'sky1').maskDataBase64).toBeNull();
    expect(stripped.exposure).toBe(0.5);

    const skyOf = (photo: string): Found[] => [{ id: 'sky1', selection: outline(`data:image/png;base64,${photo}`) }];
    const onA = withOutlines(stripped.masks, skyOf('A'));
    const onB = withOutlines(stripped.masks, skyOf('B'));
    expect(params(onA, 'sky1').maskDataBase64).toBe('data:image/png;base64,A');
    expect(params(onB, 'sky1').maskDataBase64).toBe('data:image/png;base64,B');
    expect(params(onA, 'sky1')).not.toHaveProperty('found');
    expect(params(onA, 'sky1').agSelection).toBe('sky');
  });

  it('a selection that fails leaves only its own mask empty', () => {
    const asks = asksOf(presetMasks());
    const stripped = withoutOutlines({ masks: presetMasks() }, asks).masks;
    const found: Found[] = [
      { id: 'sky1', selection: null },
      { id: 'subj', selection: outline('data:image/png;base64,S') },
    ];
    const masks = withOutlines(stripped, found);
    expect(params(masks, 'sky1').maskDataBase64).toBeNull();
    expect(params(masks, 'subj').maskDataBase64).toBe('data:image/png;base64,S');
    expect(masks[0].adjustments).toEqual({ exposure: -1 });
  });

  it('an outline already there is not replaced', () => {
    const masks = presetMasks();
    const kept = withOutlines(masks, [{ id: 'sky1', selection: outline('data:image/png;base64,NEW') }]);
    expect(params(kept, 'sky1').maskDataBase64).toBe(FIRST_PHOTO_SKY);
  });

  it('hand-made masks are unchanged byte for byte', () => {
    const preset = { masks: presetMasks() };
    const asks = asksOf(preset.masks);
    const stripped = withoutOutlines(preset, asks).masks;
    const resolved = withOutlines(stripped, [{ id: 'sky1', selection: outline('data:image/png;base64,X') }]);
    for (const masks of [stripped, resolved]) {
      expect(JSON.stringify(masks.slice(2))).toBe(JSON.stringify(handMade()));
      expect(JSON.stringify(params(masks, 'grad'))).toBe(JSON.stringify(params(presetMasks(), 'grad')));
    }
  });

  it('a preset without AI masks is the same preset, untouched', () => {
    const preset = { exposure: 0.3, masks: handMade() };
    const json = JSON.stringify(preset);
    const asks = asksOf(preset.masks);
    expect(asks).toEqual([]);
    expect(withoutOutlines(preset, asks)).toBe(preset);
    expect(JSON.stringify(preset)).toBe(json);
    const noMasks = { exposure: 0.3 };
    expect(withoutOutlines(noMasks, asksOf(undefined))).toBe(noMasks);
  });

  it('their own AI masks and selections with nothing to find from are carried as they are', () => {
    expect(wantedOf({ parameters: { maskDataBase64: FIRST_PHOTO_SKY } })).toBeNull();
    expect(wantedOf({ parameters: { agSelection: 'text', agWords: '  ' } })).toBeNull();
    expect(wantedOf({ parameters: { agSelection: 'face', agParts: [] } })).toBeNull();
    expect(wantedOf({ parameters: { agSelection: 'object', agClicks: [{ x: 1, y: 1, positive: true }] } })).toBeNull();
    expect(
      wantedOf({ parameters: { agSelection: 'object', agClicks: [], maskDataBase64: FIRST_PHOTO_SKY } }),
    ).toBeNull();
  });

  it('an Object keeps its clicks and box, and the size of the photo they were made on', () => {
    const clicks = [{ x: 10, y: 20, positive: true }];
    const bounds = { startX: 1, startY: 2, endX: 3, endY: 4 };
    expect(
      wantedOf({
        parameters: {
          agSelection: 'object',
          agClicks: clicks,
          agBounds: bounds,
          maskDataBase64: pngOf(6000, 4000),
          orientationSteps: 1,
        },
      }),
    ).toEqual({ kind: 'object', clicks, bounds, from: { width: 6000, height: 4000, orientationSteps: 1 } });
  });

  it('reads a PNG size from its header', () => {
    expect(pngSize(pngOf(6048, 4024))).toEqual({ width: 6048, height: 4024 });
    expect(pngSize('data:image/jpeg;base64,AAAA')).toBeNull();
    expect(pngSize('data:image/png;base64,AAAA')).toBeNull();
  });
});
