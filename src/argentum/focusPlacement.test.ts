// Where the camera's focus points land on the photo as the editor shows it (argentum/focusPlacement.ts).
import { describe, expect, it } from 'vitest';
import type { FocusPoint } from './ag.generated';
import type { Adjustments } from '../utils/adjustments';
import { hiddenByTransform, placeOnPhoto, type PlacedPoint } from './focusPlacement';

/** A point by its centre and size, fractions of the upright picture. */
const at = (cx: number, cy: number, w = 0.1, h = 0.1): FocusPoint => ({
  x: cx - w / 2,
  y: cy - h / 2,
  w,
  h,
  state: 'inFocus',
});
const centre = (p: PlacedPoint) => [p.x + p.w / 2, p.y + p.h / 2].map((v) => Math.round(v * 1000) / 1000);
/** Guided perspective with `lines` guides drawn. */
const guided = (enabled: boolean, lines: number) =>
  ({ guidedPerspective: { enabled, lines: Array(lines).fill({}), autoCrop: false } }) as Partial<Adjustments>;
const whole = (width: number, height: number) => ({ x: 0, y: 0, width, height });

describe('focus points placed on the photo', () => {
  it('lands where the file says when nothing is changed, at the overlay’s size', () => {
    const [p] = placeOnPhoto([at(0.75, 0.25)], {}, { width: 6000, height: 4000 }, whole(6000, 4000), {
      width: 600,
      height: 400,
    });
    expect(centre(p)).toEqual([450, 100]);
    expect([p.w, p.h]).toEqual([60, 40]);
    expect(p.angle).toBe(0);
  });

  it('turns with the photo: a quarter turn clockwise takes the right of the middle below it', () => {
    const turned = { width: 4000, height: 6000 };
    const [p] = placeOnPhoto([at(0.75, 0.5, 0.1, 0.05)], { orientationSteps: 1 }, turned, whole(4000, 6000), {
      width: 400,
      height: 600,
    });
    expect(centre(p)).toEqual([200, 450]);
    // Its box turns too: wide becomes tall.
    expect([Math.round(p.w), Math.round(p.h)]).toEqual([20, 60]);
  });

  it('mirrors with the photo', () => {
    const size = { width: 1000, height: 1000 };
    const [h] = placeOnPhoto([at(0.2, 0.3)], { flipHorizontal: true }, size, whole(1000, 1000), size);
    const [v] = placeOnPhoto([at(0.2, 0.3)], { flipVertical: true }, size, whole(1000, 1000), size);
    expect(centre(h)).toEqual([800, 300]);
    expect(centre(v)).toEqual([200, 700]);
  });

  it('straightens about the picture’s centre, clockwise, and is drawn at the same angle', () => {
    const size = { width: 1000, height: 1000 };
    const [p] = placeOnPhoto([at(0.75, 0.5)], { rotation: 90 }, size, whole(1000, 1000), size);
    expect(centre(p)).toEqual([500, 750]);
    expect(p.angle).toBe(90);
  });

  it('follows the crop: the cropped part fills the overlay', () => {
    const [p] = placeOnPhoto(
      [at(0.5, 0.5)],
      {},
      { width: 6000, height: 4000 },
      { x: 3000, y: 2000, width: 3000, height: 2000 },
      { width: 300, height: 200 },
    );
    // The middle of the picture is the crop's top-left corner.
    expect(centre(p)).toEqual([0, 0]);
    expect([p.w, p.h]).toEqual([60, 40]);
  });
});

describe('focus points hidden by Transform', () => {
  it('only while it changes the perspective', () => {
    expect(hiddenByTransform({})).toBe(false);
    expect(hiddenByTransform({ transformVertical: 0, transformScale: 100 })).toBe(false);
    expect(hiddenByTransform({ transformVertical: 12 })).toBe(true);
    expect(hiddenByTransform({ transformScale: 90 })).toBe(true);
    expect(hiddenByTransform(guided(true, 1))).toBe(false);
    expect(hiddenByTransform(guided(true, 2))).toBe(true);
    expect(hiddenByTransform(guided(false, 2))).toBe(false);
  });
});
