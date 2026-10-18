// The photo. It tells the store its size in screen pixels and draws the latest
// frame; a click toggles between fitted and 100% at the point clicked, and a
// drag pans while zoomed in.

import { useEffect, useRef, type PointerEvent } from 'react';

import { text } from '../i18n';
import { useShell, useShellStore } from '../ShellContext';
import { viewOf } from '../store';

export function Viewer() {
  const store = useShellStore();
  const box = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const drag = useRef<{ x: number; y: number; moved: boolean } | null>(null);
  const frame = useShell((s) => s.frame);
  const hasPhoto = useShell((s) => s.photo !== null);

  useEffect(() => {
    const el = box.current;
    if (!el) return;
    const observer = new ResizeObserver(([entry]) => {
      const dpr = window.devicePixelRatio || 1;
      const { width, height } = entry.contentRect;
      store.getState().setViewSize(Math.round(width * dpr), Math.round(height * dpr));
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, [store]);

  useEffect(() => {
    const c = canvas.current;
    const size = store.getState().viewSize;
    if (!c || !size) return;
    c.width = size.width;
    c.height = size.height;
    const ctx = c.getContext('2d')!;
    ctx.clearRect(0, 0, c.width, c.height);
    if (frame?.image) ctx.drawImage(frame.image, 0, 0, c.width, c.height);
  }, [frame, store]);

  /** The photo point under a screen point, 0..1. */
  const photoPoint = (clientX: number, clientY: number) => {
    const s = store.getState();
    const view = viewOf(s);
    const r = box.current!.getBoundingClientRect();
    if (!view || !s.photo) return { x: 0.5, y: 0.5 };
    const dpr = window.devicePixelRatio || 1;
    return {
      x: view.centerX + ((clientX - r.left) * dpr - view.width / 2) / (view.zoom * s.photo.width),
      y: view.centerY + ((clientY - r.top) * dpr - view.height / 2) / (view.zoom * s.photo.height),
    };
  };

  const down = (e: PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    e.currentTarget.setPointerCapture(e.pointerId);
    drag.current = { x: e.clientX, y: e.clientY, moved: false };
  };
  const move = (e: PointerEvent<HTMLDivElement>) => {
    const d = drag.current;
    const s = store.getState();
    if (!d || s.zoom === 'fit' || !s.photo) return;
    if (!d.moved && Math.hypot(e.clientX - d.x, e.clientY - d.y) < 3) return;
    d.moved = true;
    const dpr = window.devicePixelRatio || 1;
    const view = viewOf(s)!;
    s.setCenter(
      view.centerX - ((e.clientX - d.x) * dpr) / (view.zoom * s.photo.width),
      view.centerY - ((e.clientY - d.y) * dpr) / (view.zoom * s.photo.height),
    );
    d.x = e.clientX;
    d.y = e.clientY;
  };
  const up = (e: PointerEvent<HTMLDivElement>) => {
    const d = drag.current;
    drag.current = null;
    if (!d || d.moved) return;
    const s = store.getState();
    if (s.zoom === 'fit') s.setZoom(1, photoPoint(e.clientX, e.clientY));
    else s.setZoom('fit');
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
