/**
 * Where a radial mask's full effect ends. Ours.
 *
 * RapidRAW draws one ellipse, the outer edge, where the mask has faded to
 * nothing. How far in the fade reaches is the Feather slider, and the only
 * way to see it was the red mask preview, which their canvas hides while any
 * slider is being dragged. So the slider moved something nobody could see.
 *
 * This draws the inner edge as a second, solid ellipse: full effect inside it,
 * none outside theirs, the fade between. Solid for full and dashed for none,
 * as the linear mask's lines are. It follows the Feather slider as it moves.
 *
 * WHILE THEIR ELLIPSE IS DRAGGED
 *
 * Their canvas keeps a drag to itself until release; the stored mask does not
 * move until then. So the outer ellipse is read live from their Konva stage:
 * the shape their Transformer is attached to, which is the selected radial
 * mask's ellipse, through its absolute transform onto the screen. When it is
 * not there, the stored mask is used, which is the same ellipse at rest.
 *
 * Drawn only, never pressed: their canvas still owns the ellipse and its
 * handles. In the masks panel and the AI panel, which share their canvas.
 */

import { useEffect, useState } from 'react';
import { createPortal } from 'react-dom';
import Konva from 'konva';
import { useEditorStore } from '../store/useEditorStore';
import { useUIStore } from '../store/useUIStore';
import { Mask } from '../components/panel/right/Masks';
import { cropFrame, findSelectedSubMask, selectedSubMaskId } from './linearEdges';
import { PhotoLayout, usePhotoLayer } from './photoLayer';

/** An ellipse in the overlay svg's pixels; `angle` in degrees. */
interface Outline {
  cx: number;
  cy: number;
  rx: number;
  ry: number;
  angle: number;
}

/** The selected radial mask's ellipse as their stage has it right now, in our svg's pixels. */
function liveOutline(layout: PhotoLayout): Outline | null {
  for (const stage of Konva.stages) {
    const transformer = stage.findOne('Transformer') as Konva.Transformer | undefined;
    const node = transformer?.nodes()[0];
    if (!(node instanceof Konva.Ellipse)) {
      continue;
    }
    const box = stage.container().getBoundingClientRect();
    const svgBox = layout.svg.getBoundingClientRect();
    const k = box.width / (stage.width() || 1);
    const zoom = layout.zoom || 1;
    const t = node.getAbsoluteTransform();
    const toSvg = (x: number, y: number) => {
      const p = t.point({ x, y });
      return { x: (box.left + p.x * k - svgBox.left) / zoom, y: (box.top + p.y * k - svgBox.top) / zoom };
    };
    const c = toSvg(0, 0);
    const a = toSvg(node.radiusX(), 0);
    const b = toSvg(0, node.radiusY());
    return {
      cx: c.x,
      cy: c.y,
      rx: Math.hypot(a.x - c.x, a.y - c.y),
      ry: Math.hypot(b.x - c.x, b.y - c.y),
      angle: (Math.atan2(a.y - c.y, a.x - c.x) * 180) / Math.PI,
    };
  }
  return null;
}

const same = (a: Outline | null, b: Outline | null) =>
  a === b ||
  (!!a &&
    !!b &&
    Math.abs(a.cx - b.cx) < 0.05 &&
    Math.abs(a.cy - b.cy) < 0.05 &&
    Math.abs(a.rx - b.rx) < 0.05 &&
    Math.abs(a.ry - b.ry) < 0.05 &&
    Math.abs(a.angle - b.angle) < 0.01);

export default function RadialFeather() {
  const activePanel = useUIStore((s) => s.activePanel);
  const activeMaskId = useEditorStore((s) => s.activeMaskId);
  const activeAiSubMaskId = useEditorStore((s) => s.activeAiSubMaskId);
  const adjustments = useEditorStore((s) => s.adjustments);
  const showOriginal = useEditorStore((s) => s.showOriginal);

  const subMask = findSelectedSubMask(
    adjustments,
    selectedSubMaskId(activePanel, activeMaskId, activeAiSubMaskId),
    Mask.Radial,
  );
  const p = subMask?.parameters;
  const active = !!p && !p.isInitialDraw && p.radiusX > 0 && p.radiusY > 0 && !showOriginal;
  const layout = usePhotoLayer(active);

  const [live, setLive] = useState<Outline | null>(null);
  useEffect(() => {
    if (!active || !layout) {
      setLive(null);
      return;
    }
    let raf = 0;
    const tick = () => {
      const now = liveOutline(layout);
      setLive((was) => (same(was, now) ? was : now));
      raf = requestAnimationFrame(tick);
    };
    tick();
    return () => cancelAnimationFrame(raf);
  }, [active, layout]);

  const f = cropFrame();
  if (!active || !layout?.svg.parentElement || !f) {
    return null;
  }

  // As mask_generation.rs reads it: full effect out to 1 - feather of the way.
  const inner = 1 - Math.min(Math.max(p.feather ?? 0.5, 0), 1);
  if (inner <= 0) {
    return null;
  }

  const { width, height, zoom } = layout;
  const kx = width / f.width;
  const ky = height / f.height;
  const outer: Outline = live ?? {
    cx: (p.centerX - f.x) * kx,
    cy: (p.centerY - f.y) * ky,
    rx: p.radiusX * kx,
    ry: p.radiusY * ky,
    angle: p.rotation || 0,
  };
  const px = (n: number) => n / (zoom || 1);
  const ellipse = {
    cx: outer.cx,
    cy: outer.cy,
    rx: outer.rx * inner,
    ry: outer.ry * inner,
    fill: 'none',
    transform: `rotate(${outer.angle} ${outer.cx} ${outer.cy})`,
  };

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
        zIndex: 5,
      }}
    >
      <ellipse {...ellipse} stroke="rgba(0,0,0,0.5)" strokeWidth={px(3.5)} />
      <ellipse {...ellipse} stroke="white" strokeWidth={px(1.75)} />
    </svg>,
    layout.svg.parentElement,
  );
}
