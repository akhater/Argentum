/**
 * The focus points' icon and drawing. From Shoebox `src/components/FocusPoints.tsx`
 * at 580c5a6d (`FocusIcon`, `hasFocusPoints`, `FocusPoints`), so the two apps
 * look alike. Changed: the drawing takes boxes already placed on the photo
 * as the editor shows it (focusPlacement.ts), each turned with the photo, and
 * its lines stay their width at any zoom (`--af-zoom`, focusPoints.css).
 */

import { memo } from 'react';
import type { Focus, FocusState } from './ag.generated';
import type { PlacedPoint } from './focusPlacement';
import './focusPoints.css';

/** A viewfinder's corners around a small square (the focus point), filled
 *  when the points are shown. Not a bare corner frame: that reads "full screen". */
export function FocusIcon({ on, size = 16 }: { on: boolean; size?: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M3 8V5a2 2 0 0 1 2-2h3" />
      <path d="M16 3h3a2 2 0 0 1 2 2v3" />
      <path d="M21 16v3a2 2 0 0 1-2 2h-3" />
      <path d="M8 21H5a2 2 0 0 1-2-2v-3" />
      <rect x="9" y="9" width="6" height="6" rx="1" fill={on ? 'currentColor' : 'none'} />
    </svg>
  );
}

/** Something to draw: a point the photo used, or its zone (a camera's other points alone say nothing). */
export const hasFocusPoints = (f: Focus | null | undefined) => !!f && f.points.some((p) => p.state !== 'other');

/** Drawn last on top: in focus over chosen over the zone over the camera's other points. */
const ORDER: Record<FocusState, number> = { other: 0, area: 1, chosen: 2, inFocus: 3 };

/**
 * The camera's focus points over the photo, in a box the size of the photo:
 * the other points faint, the chosen ones white, those in focus red, the zone
 * given dashed. Each drawn over a dark halo, to show on bright and dark photos.
 */
export const FocusPoints = memo(function FocusPoints({ points }: { points: readonly PlacedPoint[] }) {
  const sorted = [...points].sort((a, b) => ORDER[a.state] - ORDER[b.state]);
  const box = (p: PlacedPoint) => ({
    x: p.x,
    y: p.y,
    width: p.w,
    height: p.h,
    rx: 2,
    transform: p.angle ? `rotate(${p.angle} ${p.x + p.w / 2} ${p.y + p.h / 2})` : undefined,
  });
  return (
    <svg className="focus-points" aria-hidden="true">
      {sorted.map((p, i) => (
        <g key={i} className={`af ${p.state}`}>
          <rect {...box(p)} className="halo" />
          <rect {...box(p)} className="line" />
        </g>
      ))}
    </svg>
  );
});
