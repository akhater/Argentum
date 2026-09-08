/**
 * RGB readout under the cursor. Ours.
 *
 * Answers the only question that matters about a colour module: is it right?
 * You can see that a picture changed. You cannot see whether a patch you
 * declared neutral actually came out neutral.
 *
 * WHY THE COLOUR COMES FROM RUST
 *
 * The displayed photo is not in the DOM. RapidRAW renders the editor view to a
 * native WGPU surface composited behind the webview — no canvas, no `<img>`
 * carrying the live result. Three frontend attempts failed on that, the last
 * one silently: it sampled the only large image present, which is the cached
 * `_medium.jpg` thumbnail, regenerated on save and never while a slider moves.
 * AK caught it — the same spot on a nose read 207/177/179 at correct white
 * balance and 214/189/192 at temperature -100, on a picture that had gone
 * completely blue.
 *
 * So the pixel is rendered on demand by `sample_processed_pixel`, over a
 * one-texel ROI.
 *
 * That stale thumbnail is still useful for one thing: it is positioned exactly
 * where the photo is drawn, so its bounding box maps the cursor to a point in
 * the image. Geometry from the DOM, colour from the GPU.
 */

import { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { invoke } from '@tauri-apps/api/core';
import { useEditorStore } from '../store/useEditorStore';
import { useRgbReadout } from './rgbReadoutStore';

interface Sample {
  r: number;
  g: number;
  b: number;
}

/** Largest channel gap as a share of the brightest channel. */
function castStrength({ r, g, b }: Sample): number {
  const max = Math.max(r, g, b);
  if (max < 8) {
    return 0;
  }
  return (max - Math.min(r, g, b)) / max;
}

/**
 * The element occupying the photo's place on screen.
 *
 * Only its geometry is used. It is the largest `<img>` that is not the mask
 * overlay — thumbnails top out around 480px, so the size floor separates them.
 */
function findPhotoBox(): HTMLImageElement | null {
  let best: HTMLImageElement | null = null;
  for (const img of Array.from(document.querySelectorAll('img'))) {
    const el = img as HTMLImageElement;
    if (el.alt === 'Mask Overlay' || el.naturalWidth < 600 || !el.complete) {
      continue;
    }
    if (!best || el.naturalWidth > best.naturalWidth) {
      best = el;
    }
  }
  return best;
}

export default function RgbReadout() {
  const on = useRgbReadout((s) => s.on);
  const [sample, setSample] = useState<Sample | null>(null);
  const pending = useRef(false);
  const lastSent = useRef(0);
  const onRef = useRef(false);

  useEffect(() => {
    onRef.current = on;
    if (!on) {
      setSample(null);
    }
  }, [on]);

  useEffect(() => {
    const onMove = (e: MouseEvent) => {
      if (!onRef.current) {
        return;
      }

      const box = findPhotoBox();
      if (!box) {
        setSample(null);
        return;
      }

      const rect = box.getBoundingClientRect();
      if (
        rect.width === 0 ||
        rect.height === 0 ||
        e.clientX < rect.left ||
        e.clientX > rect.right ||
        e.clientY < rect.top ||
        e.clientY > rect.bottom
      ) {
        setSample(null);
        return;
      }

      // object-contain letterboxes the picture inside the element, so map
      // against the drawn box rather than the element box.
      const scale = Math.min(rect.width / box.naturalWidth, rect.height / box.naturalHeight);
      const drawnW = box.naturalWidth * scale;
      const drawnH = box.naturalHeight * scale;
      const localX = e.clientX - rect.left - (rect.width - drawnW) / 2;
      const localY = e.clientY - rect.top - (rect.height - drawnH) / 2;
      if (localX < 0 || localY < 0 || localX >= drawnW || localY >= drawnH) {
        setSample(null);
        return;
      }

      // Each reading is a GPU render, so do not fire one per mouse event. One
      // in flight at a time, and no faster than ~12/second - fast enough to
      // feel live, slow enough not to compete with the preview.
      const now = Date.now();
      if (pending.current || now - lastSent.current < 80) {
        return;
      }
      pending.current = true;
      lastSent.current = now;

      const x = localX / drawnW;
      const y = localY / drawnH;

      invoke<[number, number, number]>('sample_processed_pixel', {
        x,
        y,
        jsAdjustments: useEditorStore.getState().adjustments,
      })
        .then(([r, g, b]) => setSample({ r, g, b }))
        .catch(() => setSample(null))
        .finally(() => {
          pending.current = false;
        });
    };

    window.addEventListener('mousemove', onMove);
    return () => window.removeEventListener('mousemove', onMove);
  }, []);

  if (!on || !sample) {
    return null;
  }

  const cast = castStrength(sample);
  // Under ~2% the channels are equal for any practical purpose — sensor noise
  // alone moves them more than that.
  const neutral = cast < 0.02;

  // Portalled to document.body: `position: fixed` resolves against the nearest
  // *transformed* ancestor, and zooming applies a transform, so the readout was
  // being clipped inside the zoomed container exactly when it was in use.
  return createPortal(
    <div
      className="fixed bottom-4 left-4 z-50 rounded-md bg-black/80 px-2.5 py-1.5 font-mono text-xs text-white backdrop-blur-sm pointer-events-none select-none"
      style={{ letterSpacing: '0.02em' }}
    >
      <span className="text-red-400">{sample.r}</span>
      {' · '}
      <span className="text-green-400">{sample.g}</span>
      {' · '}
      <span className="text-blue-400">{sample.b}</span>
      <span className={neutral ? 'ml-2 text-emerald-300' : 'ml-2 text-amber-300'}>
        {neutral ? 'neutral' : `${Math.round(cast * 100)}% cast`}
      </span>
    </div>,
    document.body,
  );
}
