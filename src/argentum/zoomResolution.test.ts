// The resolution the editor renders at when the zoom settles: up when zooming
// in, and back down when zooming out, so fit-to-window shows a fit render and
// not the 100% frame sampled down by the display.

import { describe, expect, it } from 'vitest';
import { zoomRenderResolution } from './zoomResolution';

const FIT = 2048;
const FULL = 6960;

describe('the resolution a settled zoom renders at', () => {
  it('goes up to full size when zoomed to 100%', () => {
    expect(zoomRenderResolution(FULL, FIT)).toBe(FULL);
  });

  it('comes back to the fit size when zoomed back out, as a fresh open would render', () => {
    expect(zoomRenderResolution(FIT, FULL)).toBe(FIT);
    expect(zoomRenderResolution(4608, FULL)).toBe(4608);
  });

  it('renders nothing when the frame shown is already the size wanted', () => {
    expect(zoomRenderResolution(FIT, FIT)).toBeNull();
  });

  it('waits while the photo is loading, and renders once it is', () => {
    expect(zoomRenderResolution(FIT, Infinity)).toBeNull();
    expect(zoomRenderResolution(FIT, 0)).toBe(FIT);
  });
});
