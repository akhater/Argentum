// Photo-to-screen maths.

import { describe, expect, it } from 'vitest';

import { fitRect, fitZoom, pannedCenter, photoInFrame, photoPointAt, viewFor, visiblePart } from '../view';

const photo = { width: 6000, height: 4000 };
const screen = { width_px: 1200, height_px: 1000 };

describe('the view', () => {
  it('fits the whole photo, and never enlarges it past 100%, as the engine does not', () => {
    expect(fitZoom(photo, screen)).toBeCloseTo(0.2, 10);
    expect(fitZoom({ width: 400, height: 300 }, screen)).toBe(1);
    expect(viewFor({ width: 400, height: 300 }, screen, 'fit', { x: 0.2, y: 0.9 })).toMatchObject({
      zoom: 1,
      centerX: 0.5,
      centerY: 0.5,
    });
  });

  it('keeps a zoomed photo over the view, and centres one that fits', () => {
    const view = viewFor(photo, screen, 1, { x: 0, y: 1 })!;
    expect(view.centerX).toBeCloseTo(600 / 6000, 10);
    expect(view.centerY).toBeCloseTo(1 - 500 / 4000, 10);
    expect(viewFor(photo, { width_px: 0, height_px: 10 }, 1, { x: 0.5, y: 0.5 })).toBeNull();
  });

  it('turns screen points into photo points and drags into centres', () => {
    const view = viewFor(photo, screen, 1, { x: 0.5, y: 0.5 })!;
    expect(photoPointAt(view, photo, 600, 500)).toEqual({ x: 0.5, y: 0.5 });
    expect(photoPointAt(view, photo, 1200, 500).x).toBeCloseTo(0.5 + 600 / 6000, 10);
    expect(pannedCenter(view, photo, 600, 0).x).toBeCloseTo(0.5 - 600 / 6000, 10);
  });

  it('says what part of the photo is on screen, and where the photo is in a frame', () => {
    const view = viewFor(photo, screen, 1, { x: 0.5, y: 0.5 })!;
    const part = visiblePart(view, photo);
    expect(part.width).toBeCloseTo(0.2, 10);
    expect(part.x).toBeCloseTo(0.4, 10);
    const fitted = viewFor(photo, screen, 'fit', { x: 0.5, y: 0.5 })!;
    // At 0.2 the photo is 1200 x 800, in a frame half the view's size.
    expect(photoInFrame(fitted, photo, { width_px: 600, height_px: 500 })).toEqual({
      x_px: 0,
      y_px: 50,
      width_px: 600,
      height_px: 400,
    });
    expect(fitRect({ width_px: 300, height_px: 200 }, { width_px: 150, height_px: 150 })).toEqual({
      x_px: 0,
      y_px: 25,
      width_px: 150,
      height_px: 100,
    });
  });
});
