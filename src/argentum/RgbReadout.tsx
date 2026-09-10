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
 * The geometry still has to come from the DOM, though — see `findPhotoBox`.
 * Colour from the GPU, position from the page.
 */

import { ag } from './ag';
import { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { listen } from '@tauri-apps/api/event';
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
 * The photo's box on screen.
 *
 * This used to hunt for the largest `<img>` in the page. That was never sound:
 * with the GPU renderer on there is no `<img>` for the photo at all — it is a
 * native surface composited behind the webview — so what it actually latched
 * onto was the cached `_medium.jpg` thumbnail, which is in the tree only
 * sometimes. Clear the thumbnail cache and the readout goes silent, with no
 * error to explain why.
 *
 * What is always present is the overlay `<svg>` the editor lays over the photo
 * for masks and crop handles. `ImageCanvas` sizes it in pixels to the drawn
 * image, and it sits inside the pan/zoom transform, so its bounding box *is*
 * the photo at the current zoom and pan. Every other svg in the tree is sized
 * in percentages or not at all, which is what tells them apart.
 *
 * Its rect is the drawn image exactly, so there is no letterboxing to undo.
 */
function findPhotoBox(): DOMRect | null {
  let best: DOMRect | null = null;
  for (const el of Array.from(document.querySelectorAll('svg'))) {
    const { position, width, height } = el.style;
    if (position !== 'absolute' || !width.endsWith('px') || !height.endsWith('px')) {
      continue;
    }
    const rect = el.getBoundingClientRect();
    if (rect.width < 1 || rect.height < 1) {
      continue;
    }
    if (!best || rect.width * rect.height > best.width * best.height) {
      best = rect;
    }
  }
  return best;
}

export default function RgbReadout() {
  const on = useRgbReadout((s) => s.on);
  const [sample, setSample] = useState<Sample | null>(null);
  const pending = useRef(false);
  const lastSent = useRef(0);
  // A forced refresh that arrived while a read was in flight, to be re-fired
  // as soon as that one lands.
  const queued = useRef(false);
  const onRef = useRef(false);

  useEffect(() => {
    onRef.current = on;
    if (!on) {
      setSample(null);
    }
  }, [on]);

  useEffect(() => {
    // Last cursor position, so the reading can be refreshed when a new frame
    // lands rather than only when the mouse moves. Without this the value shown
    // after an adjustment is from the *previous* render - the exact class of
    // stale-data bug this readout exists to catch.
    let lastX = 0;
    let lastY = 0;

    const sampleAt = (clientX: number, clientY: number, force = false) => {
      if (!onRef.current) {
        return;
      }

      const rect = findPhotoBox();
      if (!rect) {
        setSample(null);
        return;
      }

      const localX = clientX - rect.left;
      const localY = clientY - rect.top;
      if (localX < 0 || localY < 0 || localX >= rect.width || localY >= rect.height) {
        setSample(null);
        return;
      }

      // Each reading is a GPU render, so do not fire one per mouse event. One
      // in flight at a time, and no faster than ~12/second - fast enough to
      // feel live, slow enough not to compete with the preview.
      //
      // `force` marks a refresh that must actually happen — the picture changed
      // under a stationary cursor, so the displayed value is now wrong.
      //
      // It is not enough for it to skip the rate limit. If a read is already in
      // flight the forced one was simply dropped and nothing retried it, which
      // is why the readout still showed the pre-correction colour after using
      // the white balance picker: the click's own read was mid-flight when the
      // sliders moved. Remember it instead, and re-fire when the current one
      // lands.
      const now = Date.now();
      if (pending.current) {
        if (force) {
          queued.current = true;
        }
        return;
      }
      if (!force && now - lastSent.current < 80) {
        return;
      }
      pending.current = true;
      lastSent.current = now;

      const x = localX / rect.width;
      const y = localY / rect.height;

      ag<[number, number, number]>('sample_processed_pixel', {
        x,
        y,
        jsAdjustments: useEditorStore.getState().adjustments,
      })
        .then(([r, g, b]) => setSample({ r, g, b }))
        .catch(() => setSample(null))
        .finally(() => {
          pending.current = false;
          if (queued.current) {
            queued.current = false;
            sampleAt(lastX, lastY, true);
          }
        });
    };

    const onMove = (e: MouseEvent) => {
      lastX = e.clientX;
      lastY = e.clientY;
      sampleAt(e.clientX, e.clientY);
    };

    // Anything that changes the picture under a stationary cursor has to
    // re-trigger a read, or the readout keeps showing the previous render.
    //
    // Two triggers, because neither alone is enough. `wgpu-frame-ready` does not
    // fire on every path — notably not after the white balance picker, which is
    // exactly when a fresh reading matters most. Subscribing to the adjustments
    // does fire there, and has the further advantage that the values we send to
    // Rust are the new ones rather than whatever the store held a tick ago.
    let settle: number | null = null;
    const resampleSoon = () => {
      if (settle !== null) {
        window.clearTimeout(settle);
      }
      // Wait for the render to land, otherwise we ask for a pixel from a frame
      // that is still being drawn.
      settle = window.setTimeout(() => {
        settle = null;
        sampleAt(lastX, lastY, true);
      }, 120);
    };

    const unsubscribe = useEditorStore.subscribe((state: any, prev: any) => {
      if (state.adjustments !== prev.adjustments) {
        resampleSoon();
      }
    });

    const unlisten = listen('wgpu-frame-ready', () => {
      sampleAt(lastX, lastY, true);
    });

    window.addEventListener('mousemove', onMove);
    return () => {
      if (settle !== null) {
        window.clearTimeout(settle);
      }
      window.removeEventListener('mousemove', onMove);
      unsubscribe();
      unlisten.then((f) => f()).catch(() => {});
    };
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
