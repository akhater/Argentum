/**
 * The editor's zoom percent, and its conversion to the view's transform scale. Ours.
 *
 * A zoom percent is screen pixels per pixel of the photo as the editor shows
 * it: the crop, or else the whole photo turned by orientationSteps. 1 is 100%,
 * one photo pixel on one device (physical) pixel; CSS pixels are device pixels
 * divided by devicePixelRatio. The transform scale is react-zoom-pan-pinch's:
 * 1 is the shown photo fitted to the window (`baseRenderSize`, CSS pixels).
 *
 * RapidRAW (src/hooks/useEditorActions.ts handleZoomChange and the zoom
 * shortcuts in src/hooks/useKeyboardShortcuts.ts, unchanged on their main at
 * 4b6b45ff) worked the percent out against the uncropped photo while fitting the
 * crop to the window, so on a cropped photo "100%" magnified by full/crop size
 * and the readout still said 100%.
 */

export interface PixelSize {
  readonly width: number;
  readonly height: number;
}

/** 10%: the step shortcuts and the view stop here. */
export const ZOOM_MIN = 0.1;
/** 200%: the step shortcuts and the view's own zoom limit stop here. */
export const ZOOM_MAX = 2;

/** The screen's device pixels per CSS pixel. */
export const screenDpr = (): number => (typeof window !== 'undefined' ? window.devicePixelRatio || 1 : 1);

/**
 * The size, in photo pixels, of what the editor shows: the crop (stored in
 * the turned photo's pixels), else the photo turned by orientationSteps.
 * Null until the photo's size is known.
 */
export function shownImageSize(
  photo: PixelSize | null | undefined,
  crop: PixelSize | null | undefined,
  orientationSteps: number | null | undefined,
): PixelSize | null {
  if (!photo?.width || !photo?.height) return null;
  if (crop) return { width: crop.width, height: crop.height };
  const turned = (orientationSteps || 0) % 2 === 1;
  return turned ? { width: photo.height, height: photo.width } : { width: photo.width, height: photo.height };
}

/** What the zoom works from, as the editor's store holds it. */
export interface ZoomView {
  readonly selectedImage: PixelSize | null | undefined;
  readonly adjustments: { readonly crop?: PixelSize | null; readonly orientationSteps?: number | null };
  /** The shown photo on screen now, CSS pixels. */
  readonly displaySize: PixelSize;
  /** The shown photo fitted to the window, CSS pixels (transform scale 1). */
  readonly baseRenderSize: PixelSize;
}

export const shownImageSizeOf = (view: ZoomView): PixelSize | null =>
  shownImageSize(view.selectedImage, view.adjustments.crop, view.adjustments.orientationSteps);

/** CSS pixels per photo pixel when fitted, or null while either size is unknown. */
function fittedCssPerPhotoPx(fitted: PixelSize, shown: PixelSize | null): number | null {
  if (!shown || !(shown.width > 0) || !(shown.height > 0) || !(fitted.width > 0) || !(fitted.height > 0)) {
    return null;
  }
  // The side that limits the fit, as their handleZoomChange chose it: the two
  // aspects only differ for the moment the fitted size lags a crop change.
  return shown.width / shown.height > fitted.width / fitted.height
    ? fitted.width / shown.width
    : fitted.height / shown.height;
}

/** The zoom percent of the shown photo drawn `drawnWidth_css` wide, or null while unknown. */
export function zoomPercentOf(drawnWidth_css: number, shown: PixelSize | null, dpr: number): number | null {
  return shown && shown.width > 0 && drawnWidth_css > 0 ? (drawnWidth_css * dpr) / shown.width : null;
}

/** The zoom percent on screen now, or null while unknown. */
export const zoomPercentNow = (view: ZoomView, dpr: number): number | null =>
  zoomPercentOf(view.displaySize.width, shownImageSizeOf(view), dpr);

/** The zoom percent of the fitted view, or null while unknown. */
export function fitZoomPercent(view: ZoomView, dpr: number): number | null {
  const fitted = fittedCssPerPhotoPx(view.baseRenderSize, shownImageSizeOf(view));
  return fitted === null ? null : fitted * dpr;
}

/** The transform scale that shows the photo at `percent`, unclamped; 1 while the sizes are unknown. */
export function transformScaleAt(percent: number, fitted: PixelSize, shown: PixelSize | null, dpr: number): number {
  const fittedScale = fittedCssPerPhotoPx(fitted, shown);
  return fittedScale === null ? 1 : percent / dpr / fittedScale;
}

/**
 * The transform scale the zoom setter applies for `percent`, or for the fitted
 * view. Fit is scale 1 whatever the photo's size. A percent is kept between
 * 10% and 2 CSS pixels per photo pixel, as their setter kept it (so 200% on a
 * 1x screen, 300% on a 1.5x one).
 */
export function transformScaleForZoom(percent: number | 'fit', view: ZoomView, dpr: number): number {
  if (percent === 'fit') return 1;
  const kept = Math.max(ZOOM_MIN, Math.min(ZOOM_MAX * dpr, percent));
  return transformScaleAt(kept, view.baseRenderSize, shownImageSizeOf(view), dpr);
}

/** The view's own zoom limits (wheel, pinch), as transform scales; their Editor.tsx transformConfig. */
export function viewScaleLimits(fitted: PixelSize, shown: PixelSize | null, dpr: number) {
  if (fittedCssPerPhotoPx(fitted, shown) === null) return { minScale: 0.1, maxScale: 20 };
  // Never above the fitted view: on a very large photo 10% can still be larger than fit.
  const minScale = Math.min(1, transformScaleAt(ZOOM_MIN, fitted, shown, dpr));
  const maxScale = transformScaleAt(ZOOM_MAX, fitted, shown, dpr);
  return { minScale: Math.max(0.1, minScale), maxScale: Math.max(20, maxScale) };
}

export type ZoomShortcut = 'zoom_in_step' | 'zoom_out_step' | 'zoom_in' | 'zoom_out' | 'cycle_zoom';

/**
 * Where a zoom shortcut goes from the zoom on screen (`now`) and the fitted
 * view's (`fit`), both percents or null while unknown. Their rules, from
 * useKeyboardShortcuts.ts: steps of 10 points or x1.2 between 10% and 200%;
 * cycle goes fit -> twice fit (when under 100%) -> 100% -> fit.
 */
export function zoomAfterShortcut(shortcut: ZoomShortcut, now: number | null, fit: number | null): number | 'fit' {
  const current = now ?? 1;
  switch (shortcut) {
    case 'zoom_in_step':
      return Math.min(current + 0.1, ZOOM_MAX);
    case 'zoom_out_step':
      return Math.max(current - 0.1, ZOOM_MIN);
    case 'zoom_in':
      return Math.min(current * 1.2, ZOOM_MAX);
    case 'zoom_out':
      return Math.max(current / 1.2, ZOOM_MIN);
    case 'cycle_zoom': {
      const nowPoints = now === null ? 100 : Math.round(now * 100);
      const fitPoints = fit === null ? 100 : Math.round(fit * 100);
      const doubleFit = fitPoints * 2;
      if (Math.abs(nowPoints - fitPoints) < 5) return doubleFit < 100 ? doubleFit / 100 : 1;
      if (Math.abs(nowPoints - doubleFit) < 5 && doubleFit < 100) return 1;
      return 'fit';
    }
  }
}
