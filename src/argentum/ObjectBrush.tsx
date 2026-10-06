/**
 * The object brush on the canvas. Ours.
 *
 * Active while the selected mask component is an object brush (see
 * `objectMask.ts`). Strokes are caught before their canvas sees them, drawn
 * here, and on release sent to Rust together with the earlier strokes, so each
 * stroke refines the same selection rather than starting a new one.
 *
 * WHY IT INTERCEPTS RATHER THAN HOOKS IN
 *
 * The component is a Subject mask as far as RapidRAW is concerned, so their
 * canvas would answer a drag with a box. Changing what their canvas does with
 * a drag means a hook in `ImageCanvas.tsx`, whose two anchors belong to the
 * white balance picker. Instead a capturing listener on the window takes the
 * press first, when it lands on their mask stage, and their handler never
 * hears it — no press, so no box. Moves and releases still reach them; with
 * no press their handlers ignore both.
 *
 * WHERE IT DRAWS
 *
 * Into the layer that holds the photo's overlay svg, sized like that svg. That
 * layer is inside their pan and zoom transform and inside the editor's
 * clipping, so strokes follow the photo and never spill over the panels,
 * without any geometry of ours.
 */

import { useCallback, useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { Loader2, Paintbrush, RotateCcw } from 'lucide-react';
import { toast } from 'react-toastify';
import { useEditorStore } from '../store/useEditorStore';
import { useUIStore } from '../store/useUIStore';
import { useEditorActions } from '../hooks/useEditorActions';
import { Panel } from '../components/ui/AppProperties';
import { ToolType } from '../components/panel/right/Masks';
import { Adjustments, MaskContainer } from '../utils/adjustments';
import { findPhotoSvg } from './photoBox';
import { useAgTranslation } from './locales';
import {
  ObjectStroke,
  activeObjectMask,
  adoptObjectMasks,
  cropFrame,
  findObjectMask,
  segmentStrokes,
} from './objectMask';

const MIN_SIZE = 5;
const MAX_SIZE = 200;

/** A stroke being drawn, in the svg's own pixels. */
interface Drawing {
  points: { x: number; y: number }[];
  /** Radius in svg pixels. */
  radius: number;
  exclude: boolean;
  /** The mask component it belongs to. */
  subMaskId: string;
}

type SubMaskParameters = Record<string, unknown>;

function withSubMask(
  prev: Adjustments,
  id: string,
  update: (parameters: SubMaskParameters) => SubMaskParameters,
): Adjustments {
  return {
    ...prev,
    masks: (prev.masks || []).map((c: MaskContainer) => ({
      ...c,
      subMasks: c.subMasks.map((sm) => (sm.id === id ? { ...sm, parameters: update(sm.parameters || {}) } : sm)),
    })),
  };
}

/** The svg's size in its own pixels, from the style their canvas gives it. */
function svgSize(svg: SVGSVGElement) {
  return { width: parseFloat(svg.style.width) || 0, height: parseFloat(svg.style.height) || 0 };
}

/**
 * A stroke worth storing: points no closer than a quarter of the brush, to a
 * tenth of a pixel. The mask is saved with the photo's edits, and a few
 * hundred raw pointer positions per stroke would bloat that file for nothing
 * SAM can use.
 */
function simplify(points: { x: number; y: number }[], radius: number) {
  const gap = Math.max(1, radius / 4);
  const round = (v: number) => Math.round(v * 10) / 10;
  const kept = [points[0]];
  for (const p of points.slice(1)) {
    const last = kept[kept.length - 1];
    if (Math.hypot(p.x - last.x, p.y - last.y) >= gap) {
      kept.push(p);
    }
  }
  const end = points[points.length - 1];
  if (kept[kept.length - 1] !== end) {
    kept.push(end);
  }
  return kept.map((p) => ({ x: round(p.x), y: round(p.y) }));
}

/** Is this press aimed at their mask stage? */
function onMaskStage(target: EventTarget | null): boolean {
  return target instanceof Element && !!target.closest('.konvajs-content');
}

/**
 * The nearest ancestor that clips: the editor's visible area. The bar is
 * placed against this rather than the photo, which is larger than the screen
 * once zoomed in.
 */
function clippingAncestor(el: Element): Element | null {
  for (let p = el.parentElement; p; p = p.parentElement) {
    const { overflow, overflowX, overflowY } = getComputedStyle(p);
    if ([overflow, overflowX, overflowY].some((o) => o === 'hidden' || o === 'clip')) {
      return p;
    }
  }
  return null;
}

export default function ObjectBrush() {
  const agT = useAgTranslation();
  const { setAdjustments } = useEditorActions();
  const setEditor = useEditorStore((s) => s.setEditor);
  const activePanel = useUIStore((s) => s.activePanel);
  const activeMaskId = useEditorStore((s) => s.activeMaskId);
  const adjustments = useEditorStore((s) => s.adjustments);
  const brushSettings = useEditorStore((s) => s.brushSettings);
  const generating = useEditorStore((s) => s.isGeneratingAiMask);

  const size = brushSettings?.size ?? 50;
  const subMask = activePanel === Panel.Masks ? findObjectMask(adjustments, activeMaskId) : null;
  const active = !!subMask;
  const strokeCount = (subMask?.parameters?.objectStrokes as ObjectStroke[] | undefined)?.length ?? 0;

  const [svg, setSvg] = useState<SVGSVGElement | null>(null);
  const [barAt, setBarAt] = useState<{ left: number; top: number } | null>(null);
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const drawing = useRef<Drawing | null>(null);
  const cursor = useRef<{ x: number; y: number } | null>(null);
  const pending = useRef<{ subMaskId: string; strokes: ObjectStroke[]; local: Drawing[] } | null>(null);
  const inFlight = useRef(false);
  const again = useRef(false);
  const sizeRef = useRef(size);
  // Repaint on the next frame. Set by anything that changes what is drawn, so
  // an idle brush costs nothing per frame.
  const dirty = useRef(true);
  if (sizeRef.current !== size) {
    dirty.current = true;
  }
  sizeRef.current = size;

  // Whatever their panel creates as `object` becomes a Subject mask with
  // strokes, before anything else gets to draw it.
  useEffect(
    () =>
      useEditorStore.subscribe((state, prev) => {
        if (state.adjustments === prev.adjustments) {
          return;
        }
        const adopted = adoptObjectMasks(state.adjustments);
        if (adopted) {
          setAdjustments(adopted);
        }
      }),
    [setAdjustments],
  );

  const lastZoom = useRef(0);
  const redraw = useCallback(() => {
    const canvas = canvasRef.current;
    const layer = canvas?.parentElement?.querySelector('svg');
    if (!canvas || !layer) {
      return;
    }
    const { width, height } = svgSize(layer as SVGSVGElement);
    const rect = layer.getBoundingClientRect();
    const zoom = width > 0 ? rect.width / width : 1;
    if (!dirty.current && zoom === lastZoom.current) {
      return;
    }
    dirty.current = false;
    lastZoom.current = zoom;
    // Enough pixels to stay sharp when zoomed in, capped where a canvas gets
    // expensive.
    const density = Math.min((window.devicePixelRatio || 1) * Math.max(zoom, 1), 4096 / Math.max(width, height, 1));
    const pw = Math.round(width * density);
    const ph = Math.round(height * density);
    if (canvas.width !== pw || canvas.height !== ph) {
      canvas.width = pw;
      canvas.height = ph;
    }
    const ctx = canvas.getContext('2d');
    if (!ctx) {
      return;
    }
    ctx.setTransform(density, 0, 0, density, 0, 0);
    ctx.clearRect(0, 0, width, height);

    const paint = (d: Drawing, alpha: number) => {
      ctx.strokeStyle = d.exclude ? `rgba(244, 63, 94, ${alpha})` : `rgba(14, 165, 233, ${alpha})`;
      ctx.fillStyle = ctx.strokeStyle;
      ctx.lineWidth = d.radius * 2;
      ctx.lineCap = 'round';
      ctx.lineJoin = 'round';
      if (d.points.length === 1) {
        ctx.beginPath();
        ctx.arc(d.points[0].x, d.points[0].y, d.radius, 0, Math.PI * 2);
        ctx.fill();
        return;
      }
      ctx.beginPath();
      d.points.forEach((p, i) => (i === 0 ? ctx.moveTo(p.x, p.y) : ctx.lineTo(p.x, p.y)));
      ctx.stroke();
    };
    pending.current?.local.forEach((d) => paint(d, 0.3));
    if (drawing.current) {
      paint(drawing.current, 0.45);
    }

    const c = cursor.current;
    if (c) {
      const radius = sizeRef.current / 2 / zoom;
      ctx.lineWidth = 1.5 / zoom;
      ctx.strokeStyle = 'rgba(255, 255, 255, 0.9)';
      ctx.beginPath();
      ctx.arc(c.x, c.y, radius, 0, Math.PI * 2);
      ctx.stroke();
      ctx.strokeStyle = 'rgba(0, 0, 0, 0.5)';
      ctx.beginPath();
      ctx.arc(c.x, c.y, radius + 1.5 / zoom, 0, Math.PI * 2);
      ctx.stroke();
    }
  }, []);

  // Find the photo's layer while active, and again whenever their canvas
  // remounts it.
  useEffect(() => {
    if (!active) {
      setSvg(null);
      return;
    }
    let frame = 0;
    let current: SVGSVGElement | null = null;
    let clip: Element | null = null;
    const tick = () => {
      if (!current || !current.isConnected) {
        current = findPhotoSvg();
        clip = current ? clippingAncestor(current) : null;
        setSvg(current);
      }
      const box = (clip ?? current)?.getBoundingClientRect();
      if (box) {
        const left = Math.round(box.left + box.width / 2);
        const top = Math.round(Math.max(box.top, 0) + 12);
        setBarAt((at) => (at && at.left === left && at.top === top ? at : { left, top }));
      }
      redraw();
      frame = requestAnimationFrame(tick);
    };
    tick();
    return () => cancelAnimationFrame(frame);
  }, [active, redraw]);

  /** Send the strokes waiting on a component, and keep going while more arrive. */
  const segment = useCallback(async () => {
    if (inFlight.current) {
      again.current = true;
      return;
    }
    inFlight.current = true;
    setEditor({ isGeneratingAiMask: true });
    try {
      do {
        again.current = false;
        const batch = pending.current;
        if (!batch) {
          break;
        }
        const taken = batch.strokes.length;
        const { adjustments: now, patchesSentToBackend } = useEditorStore.getState();
        const existing: ObjectStroke[] =
          now.masks?.flatMap((c) => c.subMasks).find((sm) => sm.id === batch.subMaskId)?.parameters?.objectStrokes ??
          [];
        const strokes = [...existing, ...batch.strokes.slice(0, taken)];
        const result = await segmentStrokes(strokes);

        // Strokes and mask land together, so one undo takes back one stroke.
        patchesSentToBackend.delete(batch.subMaskId);
        setAdjustments((prev: Adjustments) =>
          withSubMask(prev, batch.subMaskId, (p) => ({ ...p, ...result, objectStrokes: strokes })),
        );
        if (pending.current === batch) {
          batch.strokes.splice(0, taken);
          batch.local.splice(0, taken);
          if (batch.strokes.length === 0) {
            pending.current = null;
          }
        }
        dirty.current = true;
      } while (again.current || pending.current);
    } catch (e) {
      pending.current = null;
      dirty.current = true;
      toast.error(`${agT('objectBrushFailed')}: ${e}`);
    } finally {
      inFlight.current = false;
      setEditor({ isGeneratingAiMask: false });
    }
  }, [agT, setAdjustments, setEditor]);

  // The brush itself: capture the press before their stage does.
  useEffect(() => {
    if (!active || !svg) {
      return;
    }

    const local = (clientX: number, clientY: number) => {
      const rect = svg.getBoundingClientRect();
      const { width, height } = svgSize(svg);
      return {
        x: ((clientX - rect.left) * width) / rect.width,
        y: ((clientY - rect.top) * height) / rect.height,
        zoom: rect.width / (width || 1),
      };
    };

    const swallow = (e: Event) => {
      if (!onMaskStage(e.target)) {
        return false;
      }
      e.stopPropagation();
      e.preventDefault();
      return true;
    };

    const onPointerDown = (e: PointerEvent) => {
      if (e.button !== 0 || !swallow(e)) {
        return;
      }
      const target = activeObjectMask();
      if (!target) {
        return;
      }
      const p = local(e.clientX, e.clientY);
      dirty.current = true;
      drawing.current = {
        points: [{ x: p.x, y: p.y }],
        radius: sizeRef.current / 2 / p.zoom,
        exclude: e.altKey,
        subMaskId: target.id,
      };
    };

    // Their stage listens for these too; a pointerdown alone does not stop
    // the mousedown the browser sends after it.
    const onMouseDown = (e: MouseEvent) => {
      if (e.button === 0) {
        swallow(e);
      }
    };
    const onTouchStart = (e: TouchEvent) => swallow(e);

    const onPointerMove = (e: PointerEvent) => {
      const p = local(e.clientX, e.clientY);
      cursor.current = onMaskStage(e.target) || drawing.current ? { x: p.x, y: p.y } : null;
      dirty.current = true;
      const d = drawing.current;
      if (!d) {
        return;
      }
      const last = d.points[d.points.length - 1];
      // Two screen pixels apart is plenty; a long stroke stays a few hundred points.
      if (Math.hypot(p.x - last.x, p.y - last.y) * p.zoom >= 2) {
        d.points.push({ x: p.x, y: p.y });
      }
    };

    const onPointerUp = () => {
      const d = drawing.current;
      drawing.current = null;
      dirty.current = true;
      const frame = cropFrame();
      const { width, height } = svgSize(svg);
      if (!d || !frame || width <= 0 || height <= 0) {
        return;
      }
      // svg pixels to their mask coordinates: the crop's origin plus a scale.
      const kx = frame.width / width;
      const ky = frame.height / height;
      const radius = d.radius * kx;
      const stroke: ObjectStroke = {
        points: simplify(
          d.points.map((p) => ({
            x: Math.min(Math.max(frame.x + p.x * kx, frame.x), frame.x + frame.width),
            y: Math.min(Math.max(frame.y + p.y * ky, frame.y), frame.y + frame.height),
          })),
          radius,
        ),
        radius: Math.round(radius * 10) / 10,
        exclude: d.exclude,
      };
      if (!pending.current || pending.current.subMaskId !== d.subMaskId) {
        pending.current = { subMaskId: d.subMaskId, strokes: [], local: [] };
      }
      pending.current.strokes.push(stroke);
      pending.current.local.push(d);
      void segment();
    };

    // A cancelled pointer, or Escape mid-stroke, drops the stroke unsent.
    const onPointerCancel = () => {
      drawing.current = null;
      dirty.current = true;
    };

    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && drawing.current) {
        onPointerCancel();
        e.stopPropagation();
      }
    };

    const onLeave = () => {
      cursor.current = null;
      dirty.current = true;
    };

    window.addEventListener('pointerdown', onPointerDown, true);
    window.addEventListener('mousedown', onMouseDown, true);
    window.addEventListener('touchstart', onTouchStart, { capture: true, passive: false });
    window.addEventListener('pointermove', onPointerMove, true);
    window.addEventListener('pointerup', onPointerUp, true);
    window.addEventListener('pointercancel', onPointerCancel, true);
    window.addEventListener('keydown', onKeyDown, true);
    document.documentElement.addEventListener('pointerleave', onLeave);
    return () => {
      window.removeEventListener('pointerdown', onPointerDown, true);
      window.removeEventListener('mousedown', onMouseDown, true);
      window.removeEventListener('touchstart', onTouchStart, true);
      window.removeEventListener('pointermove', onPointerMove, true);
      window.removeEventListener('pointerup', onPointerUp, true);
      window.removeEventListener('pointercancel', onPointerCancel, true);
      window.removeEventListener('keydown', onKeyDown, true);
      document.documentElement.removeEventListener('pointerleave', onLeave);
      drawing.current = null;
      cursor.current = null;
    };
  }, [active, svg, segment]);

  const setSize = (value: number) =>
    setEditor((s) => ({
      brushSettings: { ...(s.brushSettings ?? { feather: 50, tool: ToolType.Brush }), size: value },
    }));

  const startOver = () => {
    if (!subMask) {
      return;
    }
    pending.current = null;
    setAdjustments((prev: Adjustments) =>
      withSubMask(prev, subMask.id, (p) => {
        // Without a start and end their overlay draws no outline, as for a
        // Subject mask nobody has drawn on yet.
        const rest = { ...p };
        for (const key of ['startX', 'startY', 'endX', 'endY']) {
          delete rest[key];
        }
        return { ...rest, maskDataBase64: null, objectStrokes: [] };
      }),
    );
  };

  if (!active || !svg?.parentElement) {
    return null;
  }

  const { width, height } = svgSize(svg);
  return (
    <>
      {createPortal(
        <canvas
          ref={canvasRef}
          style={{
            position: 'absolute',
            left: svg.style.left,
            top: svg.style.top,
            width,
            height,
            pointerEvents: 'none',
            // Their mask preview sits at 3; the stroke being painted goes on top.
            zIndex: 3,
          }}
        />,
        svg.parentElement,
      )}
      {barAt &&
        createPortal(
          <div
            className="fixed z-40 flex -translate-x-1/2 items-center gap-3 rounded-full border border-surface bg-bg-secondary/90 px-3 py-1.5 text-xs text-text-secondary shadow-lg backdrop-blur"
            style={{ left: barAt.left, top: barAt.top }}
          >
            {generating ? (
              <Loader2 size={14} className="animate-spin text-accent" />
            ) : (
              <Paintbrush size={14} className="text-accent" />
            )}
            <span className="whitespace-nowrap">
              {strokeCount === 0 ? agT('objectBrushHint') : agT('objectBrushRefine')}
            </span>
            <label className="flex items-center gap-2" data-tooltip={agT('objectBrushSizeTooltip')}>
              <span>{agT('objectBrushSize')}</span>
              <input
                className="w-20 accent-[var(--color-accent,#0ea5e9)]"
                max={MAX_SIZE}
                min={MIN_SIZE}
                onChange={(e) => setSize(Number(e.target.value))}
                type="range"
                value={Math.min(Math.max(size, MIN_SIZE), MAX_SIZE)}
              />
            </label>
            {strokeCount > 0 && (
              <button
                className="flex items-center gap-1 rounded px-1.5 py-0.5 hover:bg-surface hover:text-text-primary"
                onClick={startOver}
                type="button"
              >
                <RotateCcw size={12} />
                {agT('objectBrushStartOver')}
              </button>
            )}
          </div>,
          document.body,
        )}
    </>
  );
}
