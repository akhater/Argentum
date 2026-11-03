/**
 * Where the camera's focus points land on the photo as the editor shows it. Ours.
 *
 * `focus_points` (mods/focus_points.rs) gives them as fractions of the
 * picture upright, the frame the photo opens in. The editor then turns it in
 * this order (adjustment_utils.rs `apply_spatial_transformations`): quarter
 * turns clockwise, mirrors, the straightening angle about the centre on the
 * same canvas (imageproc's `rotate_about_center`, clockwise, as the crop
 * tool's CSS rotate shows it), then the crop. The overlay svg is the cropped
 * picture (photoBox.ts), so each point is carried through the same steps
 * into its pixels.
 *
 * Not followed: a perspective change in Transform (`hiddenByTransform`), which
 * hides the points rather than draw them in the wrong place. Lens distortion
 * correction moves them by a few pixels at most and is left out.
 */

import type { Adjustments } from '../utils/adjustments';
import type { FocusPoint, FocusState } from './ag.generated';

/** A point as drawn: its box in the overlay svg's pixels, turned `angle` degrees about its centre. */
export interface PlacedPoint {
  x: number;
  y: number;
  w: number;
  h: number;
  angle: number;
  state: FocusState;
}

/** What of the edit moves the picture. */
export interface Geometry {
  orientationSteps?: number;
  flipHorizontal?: boolean;
  flipVertical?: boolean;
  rotation?: number;
}

/** A crop in the turned picture's pixels (linearEdges.ts `cropFrame`). */
export interface Frame {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** A quarter turn clockwise, as the crate's `Point::quarter`. */
const quarter = (p: FocusPoint): FocusPoint => ({ ...p, x: 1 - (p.y + p.h), y: p.x, w: p.h, h: p.w });

/**
 * `points` on the photo as shown: `picture` is the turned picture's size
 * before the crop (the frame `crop` is in), `svg` the overlay's size.
 */
export function placeOnPhoto(
  points: readonly FocusPoint[],
  geometry: Geometry,
  picture: { width: number; height: number },
  crop: Frame,
  svg: { width: number; height: number },
): PlacedPoint[] {
  const turns = (((geometry.orientationSteps ?? 0) % 4) + 4) % 4;
  const angle = geometry.rotation ?? 0;
  const rad = (angle * Math.PI) / 180;
  const [cos, sin] = [Math.cos(rad), Math.sin(rad)];
  const kx = svg.width / (crop.width || 1);
  const ky = svg.height / (crop.height || 1);
  return points.map((point) => {
    let p = point;
    for (let i = 0; i < turns; i++) p = quarter(p);
    if (geometry.flipHorizontal) p = { ...p, x: 1 - (p.x + p.w) };
    if (geometry.flipVertical) p = { ...p, y: 1 - (p.y + p.h) };
    // Its centre, in the turned picture's pixels, straightened about the picture's centre.
    const dx = (p.x + p.w / 2 - 0.5) * picture.width;
    const dy = (p.y + p.h / 2 - 0.5) * picture.height;
    const cx = picture.width / 2 + dx * cos - dy * sin;
    const cy = picture.height / 2 + dx * sin + dy * cos;
    const w = p.w * picture.width * kx;
    const h = p.h * picture.height * ky;
    return { x: (cx - crop.x) * kx - w / 2, y: (cy - crop.y) * ky - h / 2, w, h, angle, state: p.state };
  });
}

/** The edit's keys a perspective change sets (image_processing.rs `get_geometry_params_from_json`), at rest. */
const TRANSFORM_AT_REST: Partial<Record<keyof Adjustments, number>> = {
  transformDistortion: 0,
  transformVertical: 0,
  transformHorizontal: 0,
  transformRotate: 0,
  transformAspect: 0,
  transformScale: 100,
  transformXOffset: 0,
  transformYOffset: 0,
};

/** Transform's perspective is in use, so the points are not drawn. */
export function hiddenByTransform(adjustments: Partial<Adjustments>): boolean {
  const moved = (Object.entries(TRANSFORM_AT_REST) as [keyof Adjustments, number][]).some(
    ([key, rest]) => adjustments[key] !== undefined && Number(adjustments[key]) !== rest,
  );
  const guided = adjustments.guidedPerspective;
  return moved || (!!guided?.enabled && (guided.lines?.length ?? 0) >= 2);
}
