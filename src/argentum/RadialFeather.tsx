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
 * Drawn only, never pressed: their canvas still owns the ellipse and its
 * handles. While their ellipse is being dragged it changes only on their side
 * until release, so this one catches up then.
 */

import { createPortal } from 'react-dom';
import { useEditorStore } from '../store/useEditorStore';
import { useUIStore } from '../store/useUIStore';
import { Panel } from '../components/ui/AppProperties';
import { Mask } from '../components/panel/right/Masks';
import { cropFrame, findSelectedSubMask } from './linearEdges';
import { usePhotoLayer } from './photoLayer';

export default function RadialFeather() {
  const activePanel = useUIStore((s) => s.activePanel);
  const activeMaskId = useEditorStore((s) => s.activeMaskId);
  const adjustments = useEditorStore((s) => s.adjustments);
  const showOriginal = useEditorStore((s) => s.showOriginal);

  const subMask = activePanel === Panel.Masks ? findSelectedSubMask(adjustments, activeMaskId, Mask.Radial) : null;
  const p = subMask?.parameters;
  const active = !!p && !p.isInitialDraw && p.radiusX > 0 && p.radiusY > 0 && !showOriginal;
  const layout = usePhotoLayer(active);

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
  const cx = (p.centerX - f.x) * kx;
  const cy = (p.centerY - f.y) * ky;
  const rx = p.radiusX * kx * inner;
  const ry = p.radiusY * ky * inner;
  const px = (n: number) => n / (zoom || 1);
  const ellipse = { cx, cy, rx, ry, fill: 'none', transform: `rotate(${p.rotation || 0} ${cx} ${cy})` };

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
