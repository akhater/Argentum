import { useEffect, useRef } from 'react';

import { deviceScale, fitRect, photoInFrame, type PhotoSize, type RectPx } from '../view';
import type { ShownFrame } from '../viewSlice';

/**
 * A frame's photo, fitted in the canvas's box. `overlay` draws on top of it,
 * given where the photo landed in the canvas.
 */
export function PhotoPicture({
  frame,
  photo,
  className,
  overlay,
}: {
  frame: ShownFrame | null;
  photo: PhotoSize | null;
  className?: string;
  overlay?: (ctx: CanvasRenderingContext2D, drawn: RectPx) => void;
}) {
  const canvas = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const c = canvas.current;
    if (!c) return;
    const scale = deviceScale();
    c.width = Math.max(1, Math.round(c.clientWidth * scale));
    c.height = Math.max(1, Math.round(c.clientHeight * scale));
    const ctx = c.getContext('2d')!;
    ctx.clearRect(0, 0, c.width, c.height);
    if (!frame?.image || !photo) return;
    const src = photoInFrame(frame.request.view, photo, frame.size_px);
    const drawn = fitRect(src, { width_px: c.width, height_px: c.height });
    ctx.drawImage(
      frame.image,
      src.x_px,
      src.y_px,
      src.width_px,
      src.height_px,
      drawn.x_px,
      drawn.y_px,
      drawn.width_px,
      drawn.height_px,
    );
    overlay?.(ctx, drawn);
  });
  return <canvas ref={canvas} className={className} />;
}
