import { useEffect, useRef } from 'react';

import type { ShownFrame } from '../store';

export interface PhotoRect {
  /** Where the photo is drawn in the canvas, in canvas pixels. */
  x: number;
  y: number;
  width: number;
  height: number;
}

/** The part of a frame that is photo, in the frame's own pixels. */
export function photoInFrame(frame: ShownFrame, photo: { width: number; height: number }) {
  const view = frame.view;
  if (!view) return { x: 0, y: 0, width: frame.width, height: frame.height };
  const fx = frame.width / view.width;
  const fy = frame.height / view.height;
  const x = (view.width / 2 - view.centerX * photo.width * view.zoom) * fx;
  const y = (view.height / 2 - view.centerY * photo.height * view.zoom) * fy;
  const width = photo.width * view.zoom * fx;
  const height = photo.height * view.zoom * fy;
  const x0 = Math.max(0, x);
  const y0 = Math.max(0, y);
  return {
    x: x0,
    y: y0,
    width: Math.min(frame.width, x + width) - x0,
    height: Math.min(frame.height, y + height) - y0,
  };
}

/**
 * A frame's photo, fitted in the canvas's box. `overlay` draws on top of it,
 * given where the photo landed.
 */
export function PhotoPicture({
  frame,
  photo,
  className,
  overlay,
}: {
  frame: ShownFrame | null;
  photo: { width: number; height: number } | null;
  className?: string;
  overlay?: (ctx: CanvasRenderingContext2D, rect: PhotoRect) => void;
}) {
  const canvas = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const c = canvas.current;
    if (!c) return;
    const dpr = window.devicePixelRatio || 1;
    c.width = Math.max(1, Math.round(c.clientWidth * dpr));
    c.height = Math.max(1, Math.round(c.clientHeight * dpr));
    const ctx = c.getContext('2d')!;
    ctx.clearRect(0, 0, c.width, c.height);
    if (!frame?.image || !photo) return;
    const src = photoInFrame(frame, photo);
    const scale = Math.min(c.width / src.width, c.height / src.height);
    const rect = {
      width: src.width * scale,
      height: src.height * scale,
      x: (c.width - src.width * scale) / 2,
      y: (c.height - src.height * scale) / 2,
    };
    ctx.drawImage(frame.image, src.x, src.y, src.width, src.height, rect.x, rect.y, rect.width, rect.height);
    overlay?.(ctx, rect);
  });
  return <canvas ref={canvas} className={className} />;
}
