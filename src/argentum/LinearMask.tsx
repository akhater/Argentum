/**
 * The linear mask on the canvas, as two lines. Ours.
 *
 * WHY
 *
 * RapidRAW draws a linear mask as three lines: a dotted centre line with two
 * handles, and a dashed line either side of it. The centre line is where the
 * effect is at half strength, the dashed lines are where the fade starts and
 * stops, and nothing on screen says which side gets the effect. Drawing one,
 * the drag starts at the middle of the fade and the effect lands behind it.
 * AK: "which side does what makes 0 sense".
 *
 * WHAT IT IS INSTEAD
 *
 * A solid line where the effect is full and a dashed line where it has gone,
 * each with a handle marked 100% and 0%. Drawing is dragging from where the
 * effect should start to where it should be full. Afterwards:
 *
 * - a handle moves its own end, and the lines turn to follow;
 * - a line, grabbed anywhere, slides on its own, which sets how soft the edge is;
 * - the band between them moves both.
 *
 * The mask is stored exactly as RapidRAW stores it (see `linearEdges.ts`), so
 * old masks open here and new ones open in their canvas.
 *
 * HOW IT GETS THERE WITHOUT A HOOK
 *
 * `ImageCanvas.tsx` has no anchor left, so this works as the object brush did:
 * a capturing listener on the window takes a press that lands on their mask
 * stage before their handler hears it, and lets it through when it is not on
 * one of our lines. Their stage still draws its three lines, on its own
 * canvas; while this is active that canvas is hidden. It is drawn into the
 * layer that holds the photo's overlay svg, so it follows their pan and zoom.
 *
 * Only the masks panel. The AI panel's linear masks keep their canvas.
 */

import { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { useEditorStore } from '../store/useEditorStore';
import { useUIStore } from '../store/useUIStore';
import { useEditorActions } from '../hooks/useEditorActions';
import { Panel } from '../components/ui/AppProperties';
import { Mask } from '../components/panel/right/Masks';
import { Adjustments } from '../utils/adjustments';
import { usePhotoLayer } from './photoLayer';
import {
  Edges,
  LinearParameters,
  Point,
  cropFrame,
  dot,
  edgesOf,
  findSelectedSubMask,
  handleLength,
  parametersFor,
  withParameters,
} from './linearEdges';

/** How close a press must be, in screen pixels. */
const HANDLE_HIT = 12;
const LINE_HIT = 8;
/** A press that moves less than this does not start a mask. */
const START_DRAG = 4;

type Grab = 'create' | 'full-handle' | 'none-handle' | 'full-line' | 'none-line' | 'band';

interface Drag {
  grab: Grab;
  id: string;
  start: Point;
  edges: Edges | null;
}

const CURSORS: Partial<Record<Grab, string>> = {
  'full-handle': 'grab',
  'none-handle': 'grab',
  'full-line': 'move',
  'none-line': 'move',
  band: 'move',
};

// Their stage canvas is hidden while ours is up, and its cursor is ours.
const STYLE = `
body[data-ag-linear] .konvajs-content canvas { visibility: hidden; }
body[data-ag-linear-cursor="grab"] .konvajs-content { cursor: grab !important; }
body[data-ag-linear-cursor="grabbing"] .konvajs-content { cursor: grabbing !important; }
body[data-ag-linear-cursor="move"] .konvajs-content { cursor: move !important; }
`;

function onMaskStage(target: EventTarget | null): boolean {
  return target instanceof Element && !!target.closest('.konvajs-content');
}

/** What a press at `p` (mask coordinates) would take hold of. */
function hit(p: Point, e: Edges, screenPerPixel: number): Grab | null {
  const near = (a: Point, b: Point) => Math.hypot(a.x - b.x, a.y - b.y) * screenPerPixel;
  if (near(p, e.full) <= HANDLE_HIT) return 'full-handle';
  if (near(p, e.none) <= HANDLE_HIT) return 'none-handle';
  const along = dot({ x: p.x - e.full.x, y: p.y - e.full.y }, e.dir);
  const width = dot({ x: e.none.x - e.full.x, y: e.none.y - e.full.y }, e.dir);
  if (Math.abs(along) * screenPerPixel <= LINE_HIT) return 'full-line';
  if (Math.abs(along - width) * screenPerPixel <= LINE_HIT) return 'none-line';
  if (along > 0 && along < width) return 'band';
  return null;
}

/** Where the two ends go when `grab` is dragged from `start` to `p`. */
function moved(drag: Drag, p: Point): { full: Point; none: Point; dir: Point } | null {
  const e = drag.edges;
  if (drag.grab === 'create' || !e) {
    // From where the effect starts to where it is full.
    const span = { x: drag.start.x - p.x, y: drag.start.y - p.y };
    const len = Math.hypot(span.x, span.y) || 1;
    return { full: p, none: drag.start, dir: { x: span.x / len, y: span.y / len } };
  }
  const delta = { x: p.x - drag.start.x, y: p.y - drag.start.y };
  const slide = (q: Point) => {
    const d = dot(delta, e.dir);
    return { x: q.x + e.dir.x * d, y: q.y + e.dir.y * d };
  };
  switch (drag.grab) {
    case 'full-handle':
      return { full: p, none: e.none, dir: e.dir };
    case 'none-handle':
      return { full: e.full, none: p, dir: e.dir };
    case 'full-line':
      return { full: slide(e.full), none: e.none, dir: e.dir };
    case 'none-line':
      return { full: e.full, none: slide(e.none), dir: e.dir };
    case 'band':
      return {
        full: { x: e.full.x + delta.x, y: e.full.y + delta.y },
        none: { x: e.none.x + delta.x, y: e.none.y + delta.y },
        dir: e.dir,
      };
    default:
      return null;
  }
}

export default function LinearMask() {
  const { setAdjustments } = useEditorActions();
  const activePanel = useUIStore((s) => s.activePanel);
  const activeMaskId = useEditorStore((s) => s.activeMaskId);
  const adjustments = useEditorStore((s) => s.adjustments);
  const showOriginal = useEditorStore((s) => s.showOriginal);

  const subMask = activePanel === Panel.Masks ? findSelectedSubMask(adjustments, activeMaskId, Mask.Linear) : null;
  const active = !!subMask && !showOriginal;

  const layout = usePhotoLayer(active);
  const [dragging, setDragging] = useState<Grab | null>(null);
  const subMaskRef = useRef(subMask);
  subMaskRef.current = subMask;
  const drag = useRef<Drag | null>(null);
  const pending = useRef<{ id: string; params: LinearParameters } | null>(null);
  const frame = useRef(0);

  // Hide their canvas while ours is up.
  useEffect(() => {
    if (!active) {
      return;
    }
    const style = document.createElement('style');
    style.textContent = STYLE;
    document.head.appendChild(style);
    document.body.dataset.agLinear = '1';
    return () => {
      style.remove();
      delete document.body.dataset.agLinear;
      delete document.body.dataset.agLinearCursor;
    };
  }, [active]);

  // Presses on their stage: ours if they land on a line, a handle or the band.
  useEffect(() => {
    const svg = layout?.svg;
    if (!active || !svg) {
      return;
    }

    /** A screen position in mask coordinates, and screen pixels per mask pixel. */
    const toMask = (clientX: number, clientY: number) => {
      const f = cropFrame();
      const rect = svg.getBoundingClientRect();
      if (!f || rect.width <= 0 || rect.height <= 0) {
        return null;
      }
      return {
        p: {
          x: f.x + ((clientX - rect.left) / rect.width) * f.width,
          y: f.y + ((clientY - rect.top) / rect.height) * f.height,
        },
        scale: rect.width / f.width,
      };
    };

    const write = () => {
      frame.current = 0;
      const next = pending.current;
      pending.current = null;
      if (!next) {
        return;
      }
      setAdjustments((prev: Adjustments) =>
        withParameters(prev, next.id, (p) => {
          const rest: Record<string, unknown> = { ...p, ...next.params };
          delete rest.isInitialDraw;
          return rest;
        }),
      );
    };

    let tookPress = false;

    const onPointerDown = (e: PointerEvent) => {
      if (e.button !== 0 || !onMaskStage(e.target)) {
        return;
      }
      const sm = subMaskRef.current;
      const at = toMask(e.clientX, e.clientY);
      if (!sm || !at) {
        return;
      }
      let grab: Grab | null;
      let edges: Edges | null = null;
      if (sm.parameters?.isInitialDraw) {
        grab = 'create';
      } else {
        edges = edgesOf(sm.parameters as LinearParameters);
        grab = hit(at.p, edges, at.scale);
      }
      if (!grab) {
        return;
      }
      e.stopPropagation();
      e.preventDefault();
      tookPress = true;
      drag.current = { grab, id: sm.id, start: at.p, edges };
      setDragging(grab);
      if (grab !== 'create') {
        document.body.dataset.agLinearCursor = CURSORS[grab] === 'grab' ? 'grabbing' : CURSORS[grab];
      }
    };

    // Their stage listens for these too; a pointerdown alone does not stop
    // the mousedown the browser sends after it.
    const onMouseDown = (e: MouseEvent) => {
      if (tookPress) {
        e.stopPropagation();
        e.preventDefault();
      }
    };
    const onTouchStart = (e: TouchEvent) => {
      if (tookPress) {
        e.stopPropagation();
        e.preventDefault();
      }
    };

    const onPointerMove = (e: PointerEvent) => {
      const at = toMask(e.clientX, e.clientY);
      if (!at) {
        return;
      }
      const d = drag.current;
      if (!d) {
        // Hovering: show what a press would take.
        const sm = subMaskRef.current;
        const over =
          sm && !sm.parameters?.isInitialDraw && onMaskStage(e.target)
            ? hit(at.p, edgesOf(sm.parameters as LinearParameters), at.scale)
            : null;
        const cursor = over ? CURSORS[over] : undefined;
        if (cursor) {
          document.body.dataset.agLinearCursor = cursor;
        } else {
          delete document.body.dataset.agLinearCursor;
        }
        return;
      }
      if (d.grab === 'create' && Math.hypot(at.p.x - d.start.x, at.p.y - d.start.y) * at.scale < START_DRAG) {
        return;
      }
      const ends = moved(d, at.p);
      if (!ends) {
        return;
      }
      pending.current = { id: d.id, params: parametersFor(ends.full, ends.none, ends.dir, handleLength()) };
      if (!frame.current) {
        frame.current = requestAnimationFrame(write);
      }
    };

    const onPointerUp = () => {
      tookPress = false;
      if (!drag.current) {
        return;
      }
      drag.current = null;
      setDragging(null);
      delete document.body.dataset.agLinearCursor;
      if (frame.current) {
        cancelAnimationFrame(frame.current);
      }
      write();
    };

    window.addEventListener('pointerdown', onPointerDown, true);
    window.addEventListener('mousedown', onMouseDown, true);
    window.addEventListener('touchstart', onTouchStart, { capture: true, passive: false });
    window.addEventListener('pointermove', onPointerMove, true);
    window.addEventListener('pointerup', onPointerUp, true);
    window.addEventListener('pointercancel', onPointerUp, true);
    return () => {
      window.removeEventListener('pointerdown', onPointerDown, true);
      window.removeEventListener('mousedown', onMouseDown, true);
      window.removeEventListener('touchstart', onTouchStart, true);
      window.removeEventListener('pointermove', onPointerMove, true);
      window.removeEventListener('pointerup', onPointerUp, true);
      window.removeEventListener('pointercancel', onPointerUp, true);
      if (frame.current) {
        cancelAnimationFrame(frame.current);
        frame.current = 0;
      }
      pending.current = null;
      drag.current = null;
    };
  }, [active, layout?.svg, setAdjustments]);

  const f = cropFrame();
  if (!active || !layout?.svg.parentElement || !subMask || !f || subMask.parameters?.isInitialDraw) {
    return null;
  }

  const { width, height, zoom } = layout;
  const kx = width / f.width;
  const ky = height / f.height;
  const toSvg = (p: Point) => ({ x: (p.x - f.x) * kx, y: (p.y - f.y) * ky });

  const e = edgesOf(subMask.parameters as LinearParameters);
  const full = toSvg(e.full);
  const none = toSvg(e.none);
  // Both lines run at right angles to the fade, far enough to cross the photo.
  const along = { x: -e.dir.y, y: e.dir.x };
  const reach = (width + height) * 4;
  const line = (p: Point) => ({
    x1: p.x - along.x * reach,
    y1: p.y - along.y * reach,
    x2: p.x + along.x * reach,
    y2: p.y + along.y * reach,
  });

  // Constant on screen whatever the zoom.
  const px = (n: number) => n / (zoom || 1);
  const label = (p: Point, text: string) => {
    // Beside the handle, on the side away from the other one.
    const away = text === '100%' ? -1 : 1;
    const x = p.x + e.dir.x * away * px(18);
    const y = p.y + e.dir.y * away * px(18);
    return (
      <text
        x={x}
        y={y}
        fill="white"
        stroke="rgba(0,0,0,0.65)"
        strokeWidth={px(3)}
        paintOrder="stroke"
        fontSize={px(11)}
        fontWeight={600}
        textAnchor="middle"
        dominantBaseline="middle"
      >
        {text}
      </text>
    );
  };
  const handle = (p: Point, grab: Grab) => (
    <circle cx={p.x} cy={p.y} r={px(dragging === grab ? 7 : 6)} fill="#0ea5e9" stroke="white" strokeWidth={px(2)} />
  );

  return createPortal(
    <svg
      style={{
        position: 'absolute',
        left: layout.left,
        top: layout.top,
        width,
        height,
        overflow: 'visible',
        pointerEvents: 'none',
        // Above their mask preview, which sits at 3.
        zIndex: 5,
      }}
    >
      <g strokeLinecap="round">
        {/* Full effect: solid. */}
        <line {...line(full)} stroke="rgba(0,0,0,0.5)" strokeWidth={px(3.5)} />
        <line {...line(full)} stroke="white" strokeWidth={px(1.75)} />
        {/* No effect: dashed. */}
        <line {...line(none)} stroke="rgba(0,0,0,0.5)" strokeWidth={px(3.5)} />
        <line {...line(none)} stroke="white" strokeWidth={px(1.75)} strokeDasharray={`${px(7)} ${px(5)}`} />
        {/* The drag that made it: from 0% to 100%. */}
        <line
          x1={none.x}
          y1={none.y}
          x2={full.x}
          y2={full.y}
          stroke="white"
          strokeOpacity={0.6}
          strokeWidth={px(1)}
          strokeDasharray={`${px(2)} ${px(3)}`}
        />
      </g>
      {handle(none, 'none-handle')}
      {handle(full, 'full-handle')}
      {label(none, '0%')}
      {label(full, '100%')}
    </svg>,
    layout.svg.parentElement,
  );
}
