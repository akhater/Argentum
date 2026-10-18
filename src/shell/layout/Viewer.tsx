// The photo. It tells the store its size in device pixels and draws the latest
// frame where the engine placed it in the view, never stretched; a click toggles between fitted and 100% at the point clicked, and a
// drag pans while zoomed in.

import { useEffect, useRef, type PointerEvent } from 'react';

import { text } from '../i18n';
import { useShell, useShellStore } from '../ShellContext';
import { deviceScale, pannedCenter, photoPointAt } from '../view';
import { currentView } from '../viewSlice';

export function Viewer() {
  const store = useShellStore();
  const box = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const drag = useRef<{ clientX: number; clientY: number; moved: boolean } | null>(null);
  const frame = useShell((s) => s.frame);
  const hasPhoto = useShell((s) => s.photo !== null);

  useEffect(() => {
    const el = box.current;
    if (!el) return;
    const observer = new ResizeObserver(([entry]) => {
      const { width, height } = entry.contentRect;
      store.getState().setViewSize({
        width_px: Math.round(width * deviceScale()),
        height_px: Math.round(height * deviceScale()),
      });
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, [store]);

  useEffect(() => {
    const c = canvas.current;
    const size = store.getState().viewSize_px;
    if (!c || !size) return;
    c.width = size.width_px;
    c.height = size.height_px;
    const ctx = c.getContext('2d')!;
    ctx.clearRect(0, 0, c.width, c.height);
    if (!frame?.image) return;
    const { xPx, yPx, widthPx, heightPx } = frame.placement;
    ctx.drawImage(frame.image, xPx, yPx, widthPx, heightPx);
  }, [frame, store]);

  const down = (e: PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    e.currentTarget.setPointerCapture(e.pointerId);
    drag.current = { clientX: e.clientX, clientY: e.clientY, moved: false };
  };
  const move = (e: PointerEvent<HTMLDivElement>) => {
    const d = drag.current;
    const s = store.getState();
    const view = currentView(s);
    if (!d || s.zoom === 'fit' || !s.photo || !view) return;
    if (!d.moved && Math.hypot(e.clientX - d.clientX, e.clientY - d.clientY) < 3) return;
    d.moved = true;
    const scale = deviceScale();
    s.setCenter(pannedCenter(view, s.photo, (e.clientX - d.clientX) * scale, (e.clientY - d.clientY) * scale));
    d.clientX = e.clientX;
    d.clientY = e.clientY;
  };
  const up = (e: PointerEvent<HTMLDivElement>) => {
    const d = drag.current;
    drag.current = null;
    const s = store.getState();
    const view = currentView(s);
    if (!d || d.moved || !s.photo || !view) return;
    if (s.zoom !== 'fit') return s.setZoom('fit');
    const r = box.current!.getBoundingClientRect();
    const scale = deviceScale();
    s.setZoom(1, photoPointAt(view, s.photo, (e.clientX - r.left) * scale, (e.clientY - r.top) * scale));
  };

  return (
    <div
      ref={box}
      className={hasPhoto ? 'ags-viewer' : 'ags-viewer is-empty'}
      onPointerDown={down}
      onPointerMove={move}
      onPointerUp={up}
    >
      <canvas ref={canvas} className="ags-viewer-canvas" />
      {!hasPhoto && <div className="ags-viewer-empty">{text('shell.noPhoto')}</div>}
    </div>
  );
}
