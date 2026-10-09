/**
 * The linear mask as two lines. Ours.
 *
 * RapidRAW stores a linear mask as a centre line (`startX..endY`, two points on
 * it) and `range`, the distance from that line to each edge of the fade. Full
 * effect lies on one side, none on the other, and which side is which follows
 * from the order of the two points: the side to the right of start -> end, in
 * screen coordinates, fades out.
 *
 * On screen Argentum shows it as what it does instead: a line where the effect
 * is full and a line where it has gone, with a handle on each. Dragging from
 * one handle to the other is drawing the gradient, so the effect lands where
 * the drag ends. These functions convert between the two descriptions. Nothing
 * here changes what is stored, so a mask drawn either way opens the other.
 */

import { Adjustments, MaskContainer } from '../utils/adjustments';
import { Mask, SubMask } from '../components/panel/right/Masks';
import { useEditorStore } from '../store/useEditorStore';
import { Panel } from '../components/ui/AppProperties';

export interface Point {
  x: number;
  y: number;
}

/** A point on each line, and the direction from the full line towards none. */
export interface Edges {
  full: Point;
  none: Point;
  /** Unit length. */
  dir: Point;
}

export interface LinearParameters {
  startX: number;
  startY: number;
  endX: number;
  endY: number;
  range: number;
}

/** The narrowest fade: a hard edge, but never zero, which their maths divides by. */
const MIN_RANGE = 0.5;

const sub = (a: Point, b: Point): Point => ({ x: a.x - b.x, y: a.y - b.y });
const add = (a: Point, b: Point): Point => ({ x: a.x + b.x, y: a.y + b.y });
const mul = (a: Point, k: number): Point => ({ x: a.x * k, y: a.y * k });
export const dot = (a: Point, b: Point) => a.x * b.x + a.y * b.y;
const length = (a: Point) => Math.hypot(a.x, a.y);

/** The two lines of a stored mask. */
export function edgesOf(p: LinearParameters): Edges {
  const line = { x: p.endX - p.startX, y: p.endY - p.startY };
  const len = length(line) || 1;
  // `mask_generation.rs` measures across the line along (-ly, lx): that way
  // the mask fades out, so it points from full to none.
  const dir = { x: -line.y / len, y: line.x / len };
  const mid = { x: (p.startX + p.endX) / 2, y: (p.startY + p.endY) / 2 };
  const range = Math.max(p.range || 0, MIN_RANGE);
  return { full: sub(mid, mul(dir, range)), none: add(mid, mul(dir, range)), dir };
}

/**
 * What to store for full effect at `full` fading to none at `none`.
 *
 * `fallbackDir` keeps the direction when the two coincide, a hard edge, where
 * the points alone no longer say which side is which. `handle` is half the
 * length of the stored centre line; only its direction matters to the mask, so
 * it is RapidRAW's usual length, which keeps their own handles sensible if the
 * mask is edited in their canvas.
 */
export function parametersFor(full: Point, none: Point, fallbackDir: Point, handle: number): LinearParameters {
  const across = sub(none, full);
  const len = length(across);
  const dir = len > 1e-6 ? mul(across, 1 / len) : fallbackDir;
  const mid = mul(add(full, none), 0.5);
  // The centre line runs at right angles to dir, the way edgesOf reads it back.
  const along = { x: dir.y, y: -dir.x };
  return {
    startX: mid.x - along.x * handle,
    startY: mid.y - along.y * handle,
    endX: mid.x + along.x * handle,
    endY: mid.y + along.y * handle,
    range: Math.max(len / 2, MIN_RANGE),
  };
}

/** The photo as their canvas lays it out: after quarter turns, before crop. Mask coordinates live here. */
export function cropFrame(): { x: number; y: number; width: number; height: number } | null {
  const { selectedImage, adjustments } = useEditorStore.getState();
  if (!selectedImage?.width || !selectedImage?.height) {
    return null;
  }
  const turned = (adjustments.orientationSteps || 0) % 2 === 1;
  const width = turned ? selectedImage.height : selectedImage.width;
  const height = turned ? selectedImage.width : selectedImage.height;
  const crop = adjustments.crop;
  if (!crop) {
    return { x: 0, y: 0, width, height };
  }
  const pct = crop.unit === '%';
  return {
    x: pct ? (crop.x / 100) * width : crop.x,
    y: pct ? (crop.y / 100) * height : crop.y,
    width: pct ? (crop.width / 100) * width : crop.width,
    height: pct ? (crop.height / 100) * height : crop.height,
  };
}

/** Their default length for the centre line's handles, from `ImageCanvas`. */
export function handleLength(): number {
  const frame = cropFrame();
  return frame ? Math.min(frame.width, frame.height) * 0.2 : 200;
}

/**
 * The component their canvas is editing: the masks panel's selection, or the
 * AI panel's, which keeps its own. Null in any other panel.
 */
export function selectedSubMaskId(
  panel: Panel | null,
  activeMaskId: string | null,
  activeAiSubMaskId: string | null,
): string | null {
  if (panel === Panel.Masks) {
    return activeMaskId;
  }
  if (panel === Panel.Ai) {
    return activeAiSubMaskId;
  }
  return null;
}

/** Every component, in masks and in AI patches. */
function allSubMasks(adjustments: Adjustments): SubMask[] {
  return [
    ...((adjustments.masks || []) as MaskContainer[]).flatMap((c) => c.subMasks),
    ...(adjustments.aiPatches || []).flatMap((p) => p.subMasks),
  ];
}

/** The component with this id, if it is of `type`. */
export function findSelectedSubMask(adjustments: Adjustments, id: string | null, type: Mask): SubMask | null {
  if (!id) {
    return null;
  }
  const found = allSubMasks(adjustments).find((sm) => sm.id === id);
  return found?.type === type ? found : null;
}

/** Replace one component's parameters, wherever it lives, as their `updateSubMaskLocal` does. */
export function withParameters(
  prev: Adjustments,
  id: string,
  update: (parameters: Record<string, unknown>) => Record<string, unknown>,
): Adjustments {
  const apply = (sm: SubMask) => (sm.id === id ? { ...sm, parameters: update(sm.parameters || {}) } : sm);
  return {
    ...prev,
    masks: (prev.masks || []).map((c: MaskContainer) => ({ ...c, subMasks: c.subMasks.map(apply) })),
    aiPatches: (prev.aiPatches || []).map((p) => ({ ...p, subMasks: p.subMasks.map(apply) })),
  };
}
