import { describe, expect, it } from 'vitest';
import { checkResize, resizedSizePx } from './exportResize';

const LIMIT = 400_000_000;
const photo = { widthPx: 6000, heightPx: 4000 };

describe('resizedSizePx', () => {
  it('sets the chosen edge and keeps the aspect ratio', () => {
    expect(resizedSizePx(6000, 4000, 'longEdge', 3000, false)).toEqual({ widthPx: 3000, heightPx: 2000 });
    expect(resizedSizePx(6000, 4000, 'shortEdge', 2000, false)).toEqual({ widthPx: 3000, heightPx: 2000 });
    expect(resizedSizePx(6000, 4000, 'width', 1500, false)).toEqual({ widthPx: 1500, heightPx: 1000 });
    expect(resizedSizePx(6000, 4000, 'height', 1000, false)).toEqual({ widthPx: 1500, heightPx: 1000 });
  });

  it("keeps the photo as it is when Don't enlarge is on and it is smaller", () => {
    expect(resizedSizePx(6000, 4000, 'longEdge', 9000, true)).toEqual(photo);
  });
});

describe('checkResize', () => {
  it.each(['longEdge', 'shortEdge', 'width', 'height'])('accepts a normal value in %s mode', (mode) => {
    expect(checkResize(photo, mode, 2048, false, LIMIT)).toBe('ok');
  });

  it.each([0, -5, NaN, Infinity, 1.5])('refuses %s as invalid', (value) => {
    expect(checkResize(photo, 'longEdge', value, false, LIMIT)).toBe('invalid');
  });

  it('is at the boundary the way Rust is: 400 MP is allowed, one pixel more is not', () => {
    const square = { widthPx: 1000, heightPx: 1000 };
    expect(checkResize(square, 'longEdge', 20_000, false, LIMIT)).toBe('ok');
    expect(checkResize(square, 'longEdge', 20_001, false, LIMIT)).toBe('overLimit');
  });

  it('flags an extra zero in every mode', () => {
    for (const mode of ['longEdge', 'shortEdge', 'width', 'height']) {
      expect(checkResize(photo, mode, 1_000_000, false, LIMIT)).toBe('overLimit');
    }
  });

  it("does not flag a large value when Don't enlarge keeps the photo as it is", () => {
    expect(checkResize(photo, 'longEdge', 1_000_000, true, LIMIT)).toBe('ok');
  });

  it('only checks the value when the photo size or the limit is not known', () => {
    expect(checkResize(null, 'longEdge', 1_000_000, false, LIMIT)).toBe('ok');
    expect(checkResize(photo, 'longEdge', 1_000_000, false, null)).toBe('ok');
    expect(checkResize(null, 'longEdge', 0, false, LIMIT)).toBe('invalid');
  });
});
