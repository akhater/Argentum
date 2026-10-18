// Photo-to-screen maths, all of it. A photo point is 0..1 across and down the
// photo; a screen length is in pixels and its name ends in _px. A `View` is the
// contract's: zoom in screen pixels per photo pixel, its size in screen pixels.

import type { Opened, View } from './contract';

/** The photo's size in its own pixels, as `open` reports it. */
export type PhotoSize = Pick<Opened, 'width' | 'height'>;

export interface SizePx {
  width_px: number;
  height_px: number;
}

/** A point of the photo, 0..1 across and down. */
export interface PhotoPoint {
  x: number;
  y: number;
}

/** A part of the photo, 0..1 across and down. */
export interface PhotoPart {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface RectPx {
  x_px: number;
  y_px: number;
  width_px: number;
  height_px: number;
}

/** Fitted to the view, or screen pixels per photo pixel. */
export type Zoom = 'fit' | number;

export const PHOTO_CENTER: PhotoPoint = { x: 0.5, y: 0.5 };

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/** The zoom at which the whole photo fits the view, never past 100%, as the engine fits. */
export function fitZoom(photo: PhotoSize, view_px: SizePx): number {
  return Math.min(view_px.width_px / photo.width, view_px.height_px / photo.height, 1);
}

/**
 * The view to ask the engine for, or null while there is nothing to draw
 * into. Along each axis the photo overflows, the centre is kept so the photo
 * covers the view; along the others it is centred.
 */
export function viewFor(photo: PhotoSize, view_px: SizePx, zoom: Zoom, center: PhotoPoint): View | null {
  if (view_px.width_px < 1 || view_px.height_px < 1) return null;
  const z = zoom === 'fit' ? fitZoom(photo, view_px) : zoom;
  const axis = (c: number, length_px: number, photoLength: number) => {
    const half = length_px / (2 * photoLength * z);
    return half >= 0.5 ? 0.5 : clamp(c, half, 1 - half);
  };
  return {
    zoom: z,
    centerX: axis(center.x, view_px.width_px, photo.width),
    centerY: axis(center.y, view_px.height_px, photo.height),
    width: view_px.width_px,
    height: view_px.height_px,
  };
}

/** The photo point under a point of the view, given in pixels from its top left. */
export function photoPointAt(view: View, photo: PhotoSize, x_px: number, y_px: number): PhotoPoint {
  return {
    x: view.centerX + (x_px - view.width / 2) / (view.zoom * photo.width),
    y: view.centerY + (y_px - view.height / 2) / (view.zoom * photo.height),
  };
}

/** The centre once the photo is dragged by dx_px, dy_px on screen. */
export function pannedCenter(view: View, photo: PhotoSize, dx_px: number, dy_px: number): PhotoPoint {
  return {
    x: view.centerX - dx_px / (view.zoom * photo.width),
    y: view.centerY - dy_px / (view.zoom * photo.height),
  };
}

/** The part of the photo a view shows, cut to the photo. */
export function visiblePart(view: View, photo: PhotoSize): PhotoPart {
  const width = view.width / (view.zoom * photo.width);
  const height = view.height / (view.zoom * photo.height);
  const x = Math.max(0, view.centerX - width / 2);
  const y = Math.max(0, view.centerY - height / 2);
  return { x, y, width: Math.min(1, view.centerX + width / 2) - x, height: Math.min(1, view.centerY + height / 2) - y };
}

/** Something of `content_px` shape fitted whole and centred in `box_px`. */
export function fitRect(content_px: SizePx, box_px: SizePx): RectPx {
  const scale = Math.min(box_px.width_px / content_px.width_px, box_px.height_px / content_px.height_px);
  const width_px = content_px.width_px * scale;
  const height_px = content_px.height_px * scale;
  return { x_px: (box_px.width_px - width_px) / 2, y_px: (box_px.height_px - height_px) / 2, width_px, height_px };
}

/** Where the photo lies in a frame rendered for `view`, in the frame's own pixels, cut to the frame. */
export function photoInFrame(view: View, photo: PhotoSize, frame_px: SizePx): RectPx {
  const fx = frame_px.width_px / view.width;
  const fy = frame_px.height_px / view.height;
  const x_px = (view.width / 2 - view.centerX * photo.width * view.zoom) * fx;
  const y_px = (view.height / 2 - view.centerY * photo.height * view.zoom) * fy;
  const x0 = Math.max(0, x_px);
  const y0 = Math.max(0, y_px);
  return {
    x_px: x0,
    y_px: y0,
    width_px: Math.min(frame_px.width_px, x_px + photo.width * view.zoom * fx) - x0,
    height_px: Math.min(frame_px.height_px, y_px + photo.height * view.zoom * fy) - y0,
  };
}
