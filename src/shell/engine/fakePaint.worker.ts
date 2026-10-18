// The contract fake's pixels, off the UI thread: a made-up landscape per photo
// name, painted at exactly the engine's placement for the asked view, with the
// three tools' settings applied in the crudest way that shows they work. Nothing here is image processing; it lets
// the shell be looked at before the engine is wired in.

import { HISTOGRAM_BINS } from '../contract';
import type { FramePixels } from './client';
import type { FrameLayout } from './fakeLayout';

export interface PaintMessage {
  seed: number;
  /** The photo's width over its height. */
  aspect: number;
  layout: FrameLayout;
  gains: [number, number, number];
  recover: boolean;
  sharpen: number;
}

export interface PaintReply extends FramePixels {
  bins: number[];
}

const frac = (x: number) => x - Math.floor(x);

function scene(u: number, v: number, aspect: number, seed: number, sharpen: number): [number, number, number] {
  const sunU = 0.2 + 0.6 * frac(seed * 0.131);
  const sunV = 0.22 + 0.12 * frac(seed * 0.717);
  const horizon =
    0.58 + 0.08 * frac(seed * 0.373) + 0.05 * Math.sin(u * 7 + seed) + 0.025 * Math.sin(u * 19 + seed * 2);
  const far = horizon - 0.07 + 0.04 * Math.sin(u * 4.3 + seed * 3);
  if (v < far) {
    const t = Math.pow(v / far, 1.6);
    const du = (u - sunU) * aspect;
    const dv = v - sunV;
    const d = Math.sqrt(du * du + dv * dv);
    const glow = 0.45 * Math.exp(-d * 7) + (d < 0.035 ? 7 : 0);
    return [0.06 + 0.85 * t + glow, 0.16 + 0.48 * t + 0.82 * glow, 0.48 - 0.12 * t + 0.55 * glow];
  }
  if (v < horizon) {
    const haze = (horizon - v) / (horizon - far);
    return [0.12 + 0.2 * haze, 0.16 + 0.18 * haze, 0.26 + 0.12 * haze];
  }
  // Fine texture: what capture sharpening is seen on at 100%.
  const tex = Math.sin(u * 1400 + Math.sin(v * 90) * 3) * Math.sin(v * 1100 + u * 40);
  const k = 1 + 0.18 * (1 + 1.6 * sharpen) * tex;
  const base = 0.05 + (0.12 * (v - horizon)) / (1 - horizon);
  return [base * 1.25 * k, base * 1.6 * k, base * 0.55 * k];
}

/** Highlights: plain clipping per channel, or a shoulder that keeps the colour. */
function tone(r: number, g: number, b: number, recover: boolean): [number, number, number] {
  if (!recover) return [Math.min(r, 1), Math.min(g, 1), Math.min(b, 1)];
  const m = Math.max(r, g, b);
  if (m <= 0.8) return [r, g, b];
  const s = (0.8 + 0.2 * (1 - Math.exp(-(m - 0.8) / 0.2))) / m;
  return [r * s, g * s, b * s];
}

/** Values an 8-bit channel can take. */
const LEVELS = 256;

const encode = (x: number) => Math.round((LEVELS - 1) * Math.pow(Math.max(0, Math.min(1, x)), 1 / 2.2));

function paint(job: PaintMessage): PaintReply {
  const { layout, gains } = job;
  const { widthPx: width, heightPx: height } = layout.placement;
  const data = new Uint8ClampedArray(width * height * 4);
  const bins = new Array(HISTOGRAM_BINS).fill(0);
  // Each frame pixel samples the render where the engine's would: its centre, in render pixels.
  for (let y = 0; y < height; y++) {
    const v = (layout.y0Px + (y + 0.5) * layout.perScreen) / layout.renderHeightPx;
    for (let x = 0; x < width; x++) {
      const u = (layout.x0Px + (x + 0.5) * layout.perScreen) / layout.renderWidthPx;
      const lin = scene(u, v, job.aspect, job.seed, job.sharpen);
      const [r, g, b] = tone(lin[0] * gains[0], lin[1] * gains[1], lin[2] * gains[2], job.recover);
      const i = (y * width + x) * 4;
      data[i] = encode(r);
      data[i + 1] = encode(g);
      data[i + 2] = encode(b);
      data[i + 3] = 255;
      // Rec. 709 weights on the encoded values, as the engine's histogram.
      const luma = 0.2126 * data[i] + 0.7152 * data[i + 1] + 0.0722 * data[i + 2];
      bins[Math.min(HISTOGRAM_BINS - 1, Math.floor((luma * HISTOGRAM_BINS) / LEVELS))]++;
    }
  }
  return { width, height, data, bins };
}

self.onmessage = (event: MessageEvent<PaintMessage>) => {
  const reply = paint(event.data);
  (self as unknown as Worker).postMessage(reply, [reply.data.buffer]);
};
