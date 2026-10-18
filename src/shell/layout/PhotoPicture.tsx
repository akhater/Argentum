import { useEffect, useRef } from 'react';

import { deviceScale, fitRect, type RectPx } from '../view';
import type { ShownFrame } from '../viewSlice';

/**
 * A fitted frame - the whole photo, as the engine cuts it - fitted in the
 * canvas's box. `overlay` draws on top of it, given where the photo landed.
 */
export function PhotoPicture({
  frame,
  className,
  overlay,
}: {
  frame: ShownFrame | null;
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
    if (!frame?.image) return;
    const { widthPx, heightPx } = frame.placement;
    const drawn = fitRect({ width_px: widthPx, height_px: heightPx }, { width_px: c.width, height_px: c.height });
    ctx.drawImage(frame.image, drawn.x_px, drawn.y_px, drawn.width_px, drawn.height_px);
    overlay?.(ctx, drawn);
  });
  return <canvas ref={canvas} className={className} />;
}
