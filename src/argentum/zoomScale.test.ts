// The editor's zoom percent against the photo as shown: crop and orientation (argentum/zoomScale.ts).
import { describe, expect, it } from 'vitest';
import {
  fitZoomPercent,
  shownImageSize,
  transformScaleForZoom,
  viewScaleLimits,
  zoomAfterShortcut,
  zoomPercentNow,
  type PixelSize,
  type ZoomView,
} from './zoomScale';

// The reported photo: a portrait CR3, 4640x6960, cropped to 2497x3330.
const PHOTO = { width: 4640, height: 6960 };
const CROP = { width: 2497, height: 3330 };
const WINDOW = { width: 1600, height: 1000 };

/** What the editor fits to the window: the shown photo scaled to fit `WINDOW`. */
function fitted(shown: PixelSize, box: PixelSize = WINDOW): PixelSize {
  const s = Math.min(box.width / shown.width, box.height / shown.height);
  return { width: shown.width * s, height: shown.height * s };
}

/** The editor's state at transform `scale`, as Editor.tsx sets displaySize and baseRenderSize. */
function view(crop: PixelSize | null, orientationSteps = 0, scale = 1): ZoomView {
  const shown = shownImageSize(PHOTO, crop, orientationSteps)!;
  const base = fitted(shown);
  return {
    selectedImage: PHOTO,
    adjustments: { crop, orientationSteps },
    baseRenderSize: base,
    displaySize: { width: base.width * scale, height: base.height * scale },
  };
}

/** Device pixels on screen per photo pixel, once the setter's scale is applied. */
function devicePxPerPhotoPx(v: ZoomView, scale: number, dpr: number): number {
  const shown = shownImageSize(v.selectedImage, v.adjustments.crop, v.adjustments.orientationSteps)!;
  return (v.baseRenderSize.width * scale * dpr) / shown.width;
}

/** The inherited setter (their useEditorActions.ts handleZoomChange), for the uncropped comparison. */
function inheritedSetter(zoomValue: number, fit: boolean, v: ZoomView, dpr: number): number {
  const swapped = v.adjustments.orientationSteps === 1 || v.adjustments.orientationSteps === 3;
  const w = swapped ? PHOTO.height : PHOTO.width;
  const h = swapped ? PHOTO.width : PHOTO.height;
  const base = v.baseRenderSize;
  const byWidth = w / h > base.width / base.height;
  let target = fit ? (byWidth ? base.width / w : base.height / h) : zoomValue / dpr;
  target = Math.max(0.1 / dpr, Math.min(2.0, target));
  return byWidth ? (target * w) / base.width : (target * h) / base.height;
}

describe('shownImageSize', () => {
  it('is the crop when there is one, whatever the orientation', () => {
    for (const steps of [0, 1, 2, 3]) expect(shownImageSize(PHOTO, CROP, steps)).toEqual(CROP);
  });
  it('is the photo, turned for orientationSteps 1 and 3', () => {
    expect(shownImageSize(PHOTO, null, 0)).toEqual(PHOTO);
    expect(shownImageSize(PHOTO, null, 1)).toEqual({ width: 6960, height: 4640 });
    expect(shownImageSize(PHOTO, null, 2)).toEqual(PHOTO);
    expect(shownImageSize(PHOTO, null, 3)).toEqual({ width: 6960, height: 4640 });
  });
  it('is unknown until the photo has a size', () => {
    expect(shownImageSize(null, CROP, 0)).toBeNull();
    expect(shownImageSize({ width: 0, height: 0 }, null, 0)).toBeNull();
  });
});

describe('100% is one shown photo pixel per device pixel', () => {
  for (const dpr of [1, 1.5]) {
    for (const steps of [0, 1, 2, 3]) {
      for (const crop of [null, CROP]) {
        it(`dpr ${dpr}, orientation ${steps}, ${crop ? 'cropped' : 'uncropped'}`, () => {
          const v = view(crop, steps);
          const scale = transformScaleForZoom(1, v, dpr);
          expect(devicePxPerPhotoPx(v, scale, dpr)).toBeCloseTo(1, 9);
          // And the readout, once the view is at that scale, says 100%.
          expect(zoomPercentNow(view(crop, steps, scale), dpr)).toBeCloseTo(1, 9);
        });
      }
    }
  }

  it('the reported photo: 100% draws the 3330 px crop 3330 device px tall, not 6960', () => {
    const dpr = 1.5;
    const v = view(CROP);
    const scale = transformScaleForZoom(1, v, dpr);
    expect(v.baseRenderSize.height * scale * dpr).toBeCloseTo(3330, 6);
    // The inherited setter drew it 6960/3330 = 2.09x too large.
    const before = inheritedSetter(1, false, v, dpr);
    expect((v.baseRenderSize.height * before * dpr) / 3330).toBeCloseTo(6960 / 3330, 6);
  });
});

describe('uncropped photos zoom as before', () => {
  for (const dpr of [1, 1.5]) {
    for (const steps of [0, 1, 2, 3]) {
      it(`dpr ${dpr}, orientation ${steps}`, () => {
        const v = view(null, steps);
        for (const z of [0.1, 0.25, 0.5, 1, 1.5, 2, 4]) {
          expect(transformScaleForZoom(z, v, dpr)).toBeCloseTo(inheritedSetter(z, false, v, dpr), 9);
        }
        // Fit on a photo larger than the window: scale 1, as before.
        expect(transformScaleForZoom('fit', v, dpr)).toBeCloseTo(inheritedSetter(0, true, v, dpr), 9);
      });
    }
  }
});

describe('fit', () => {
  it('is scale 1, cropped or not', () => {
    expect(transformScaleForZoom('fit', view(CROP), 1)).toBe(1);
    expect(transformScaleForZoom('fit', view(null), 1.5)).toBe(1);
  });
  it('is scale 1 for a crop smaller than half the window (the setter capped it at 200%)', () => {
    const tiny = { width: 300, height: 200 };
    expect(transformScaleForZoom('fit', view(tiny), 1)).toBe(1);
  });
  it('reads as the fitted size over the crop', () => {
    // 1000 CSS px tall for 3330 crop px.
    expect(fitZoomPercent(view(CROP), 1)).toBeCloseTo(1000 / 3330, 9);
    expect(fitZoomPercent(view(CROP), 1.5)).toBeCloseTo(1500 / 3330, 9);
    expect(fitZoomPercent(view(null), 1)).toBeCloseTo(1000 / 6960, 9);
  });
});

describe('the 200% cap', () => {
  it('the setter keeps 2 CSS px per photo px: 200% at dpr 1, 300% at dpr 1.5', () => {
    const v = view(CROP);
    expect(devicePxPerPhotoPx(v, transformScaleForZoom(4, v, 1), 1)).toBeCloseTo(2, 9);
    expect(devicePxPerPhotoPx(v, transformScaleForZoom(4, v, 1.5), 1.5)).toBeCloseTo(3, 9);
    expect(devicePxPerPhotoPx(v, transformScaleForZoom(0.01, v, 1.5), 1.5)).toBeCloseTo(0.1, 9);
  });
  it('the view stops at 200% of the crop (or 20x fit), and never above fit at the bottom', () => {
    const shown = CROP;
    const base = fitted(shown);
    for (const dpr of [1, 1.5]) {
      const { minScale, maxScale } = viewScaleLimits(base, shown, dpr);
      const at200 = (2 / dpr) * (shown.height / base.height);
      expect(maxScale).toBeCloseTo(Math.max(20, at200), 9);
      expect(minScale).toBeLessThanOrEqual(1);
    }
    // A 60000 px photo: 10% is still larger than fit, so the floor is fit.
    const huge = { width: 60000, height: 40000 };
    expect(viewScaleLimits(fitted(huge), huge, 1).minScale).toBe(1);
    expect(viewScaleLimits({ width: 0, height: 0 }, null, 1)).toEqual({ minScale: 0.1, maxScale: 20 });
  });
});

describe('shortcuts', () => {
  it('step by 10 points and by x1.2 between 10% and 200%', () => {
    expect(zoomAfterShortcut('zoom_in_step', 1, 0.3)).toBeCloseTo(1.1, 9);
    expect(zoomAfterShortcut('zoom_in_step', 1.95, 0.3)).toBe(2);
    expect(zoomAfterShortcut('zoom_out_step', 1, 0.3)).toBeCloseTo(0.9, 9);
    expect(zoomAfterShortcut('zoom_out_step', 0.15, 0.3)).toBe(0.1);
    expect(zoomAfterShortcut('zoom_in', 1, 0.3)).toBeCloseTo(1.2, 9);
    expect(zoomAfterShortcut('zoom_in', 1.9, 0.3)).toBe(2);
    expect(zoomAfterShortcut('zoom_out', 1.2, 0.3)).toBeCloseTo(1, 9);
    expect(zoomAfterShortcut('zoom_out', 0.11, 0.3)).toBe(0.1);
    expect(zoomAfterShortcut('zoom_in_step', null, null)).toBeCloseTo(1.1, 9);
  });
  it('step from the crop readout: at true 100% a step in is 110% of the crop', () => {
    const dpr = 1.5;
    const at100 = view(CROP, 0, transformScaleForZoom(1, view(CROP), dpr));
    const next = zoomAfterShortcut('zoom_in_step', zoomPercentNow(at100, dpr), fitZoomPercent(at100, dpr));
    expect(next).toBeCloseTo(1.1, 9);
  });
  it('cycle: fit -> twice fit (under 100%) -> 100% -> fit', () => {
    expect(zoomAfterShortcut('cycle_zoom', 0.3, 0.3)).toBeCloseTo(0.6, 9);
    expect(zoomAfterShortcut('cycle_zoom', 0.6, 0.3)).toBe(1);
    expect(zoomAfterShortcut('cycle_zoom', 1, 0.3)).toBe('fit');
    // Twice fit at or past 100%: fit -> 100% -> fit.
    expect(zoomAfterShortcut('cycle_zoom', 0.6, 0.6)).toBe(1);
    expect(zoomAfterShortcut('cycle_zoom', 1, 0.6)).toBe('fit');
  });
  it('cycle on the reported crop at dpr 1.5: fit is 45%, then 90%, then 100%', () => {
    const dpr = 1.5;
    const atFit = view(CROP);
    const fit = fitZoomPercent(atFit, dpr)!;
    expect(Math.round(fit * 100)).toBe(45);
    expect(zoomAfterShortcut('cycle_zoom', zoomPercentNow(atFit, dpr), fit)).toBeCloseTo(0.9, 9);
    const atDouble = view(CROP, 0, transformScaleForZoom(0.9, atFit, dpr));
    expect(zoomAfterShortcut('cycle_zoom', zoomPercentNow(atDouble, dpr), fit)).toBe(1);
  });
});
