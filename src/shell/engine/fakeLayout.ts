// How the engine makes a view of a photo - the size it renders at, the part
// of that render the view shows, and where the frame sits in the view - so
// the contract fake paints exactly the frame the engine would. Copied from
// src-tauri/engine/src/frame.rs `layout` and develop.rs `fit_px` (main
// @ cbff4ff5); tests/contract.test.ts holds it to the "placements" fixtures.

import type { Placement, View } from '../contract';
import { clamp, type PhotoSize } from '../view';

export interface FrameLayout {
  readonly placement: Placement;
  /** The render the frame is cut from, in render pixels. */
  readonly renderWidthPx: number;
  readonly renderHeightPx: number;
  /** Render pixels per device pixel: 1 up to 100%, less past it. */
  readonly perScreen: number;
  /** The top left corner of the part shown, in render pixels. */
  readonly x0Px: number;
  readonly y0Px: number;
}

/** The engine's fit: the whole photo in max_w x max_h, never enlarged, rounded to whole pixels. */
function fitPx(widthPx: number, heightPx: number, maxWPx: number, maxHPx: number): [number, number] {
  const w = Math.max(1, widthPx);
  const h = Math.max(1, heightPx);
  const scale = Math.min(maxWPx / w, maxHPx / h, 1);
  return [Math.max(1, Math.round(w * scale)), Math.max(1, Math.round(h * scale))];
}

export function frameLayout(view: View, photo: PhotoSize): FrameLayout {
  const wPx = Math.max(1, photo.width);
  const hPx = Math.max(1, photo.height);
  // The engine reads the view's floats as f32.
  const zoom = Math.fround(view.zoom);
  const [renderWidthPx, renderHeightPx] =
    zoom >= 1 ? [wPx, hPx] : fitPx(wPx, hPx, Math.max(1, Math.ceil(wPx * zoom)), Math.max(1, Math.ceil(hPx * zoom)));
  const perScreen = zoom > 1 ? 1 / zoom : 1;
  const axis = (center: number, viewPx: number, renderPx: number) => {
    const wanted = viewPx * perScreen;
    const span = Math.min(wanted, renderPx);
    const start = clamp(clamp(Math.fround(center), 0, 1) * renderPx - span / 2, 0, renderPx - span);
    const framePx = clamp(Math.round(span / perScreen), 1, viewPx);
    const offsetPx = wanted >= renderPx ? Math.floor((viewPx - framePx) / 2) : 0;
    return [start, framePx, offsetPx] as const;
  };
  const [x0Px, widthPx, xPx] = axis(view.centerX, view.width, renderWidthPx);
  const [y0Px, heightPx, yPx] = axis(view.centerY, view.height, renderHeightPx);
  return {
    placement: { xPx, yPx, widthPx, heightPx },
    renderWidthPx,
    renderHeightPx,
    perScreen,
    x0Px,
    y0Px,
  };
}
