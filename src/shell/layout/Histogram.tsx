import { useEffect, useRef } from 'react';

import { useShell } from '../ShellContext';
import { deviceScale } from '../view';

/** The engine's luminance bins, scaled to the tallest bin short of the two ends. */
export function Histogram() {
  const canvas = useRef<HTMLCanvasElement>(null);
  const bins = useShell((s) => s.histogram);

  useEffect(() => {
    const c = canvas.current;
    if (!c) return;
    const dpr = deviceScale();
    c.width = Math.round(c.clientWidth * dpr);
    c.height = Math.round(c.clientHeight * dpr);
    const ctx = c.getContext('2d')!;
    ctx.clearRect(0, 0, c.width, c.height);
    if (!bins?.length) return;
    // Clipped ends would flatten everything else; they are drawn, just not scaled to.
    const tallest = Math.max(1, ...bins.slice(1, -1));
    const w = c.width / bins.length;
    ctx.fillStyle = 'rgba(205, 205, 205, 0.75)';
    ctx.beginPath();
    ctx.moveTo(0, c.height);
    bins.forEach((n, i) => ctx.lineTo(i * w, c.height - Math.min(1, n / tallest) * c.height * 0.95));
    ctx.lineTo(c.width, c.height);
    ctx.closePath();
    ctx.fill();
    if (bins[bins.length - 1] > 0) {
      ctx.fillStyle = 'rgb(222, 184, 92)';
      ctx.fillRect(c.width - 4 * dpr, 0, 4 * dpr, 4 * dpr);
    }
  }, [bins]);

  return <canvas ref={canvas} className="ags-histogram" />;
}
