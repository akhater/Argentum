/**
 * Sharpening's settings and its mask view. Ours.
 *
 * The settings live under `agSharpen` in the photo's adjustments, and only
 * once something has been changed - every key there is part of the thumbnail
 * cache hash, so writing the defaults into every photo would rebuild the
 * library for nothing. `mods/sharpen.rs` reads them with the same defaults
 * as below; keep the two in step.
 */

import { useCallback, useEffect, useRef, useState } from 'react';
import { useEditorStore } from '../store/useEditorStore';

export interface AgSharpen {
  /** RawTherapee capture sharpening, RAW only. */
  capture: boolean;
  /** 0..100 %. */
  captureAmount: number;
  autoRadius: boolean;
  /** Gaussian sigma, full-resolution pixels, 0.4..2. */
  radius: number;
  /** RawTherapee's corner radius boost, -0.5..0.5. */
  cornerBoost: number;
  iterations: number;
  /** RawTherapee's iteration check: stop where deconvolution starts to ring. */
  iterCheck: boolean;
  /** darktable's sharpen amount, as a percentage of its 0..2. */
  amount: number;
  /** darktable's radius: gaussian sigma, full-resolution pixels. */
  usmRadius: number;
  /** darktable's threshold, L* units. */
  threshold: number;
  autoContrast: boolean;
  /** RawTherapee's contrast threshold, 0..200. */
  contrast: number;
}

/** RawTherapee's capture sharpening defaults and darktable's sharpen preset. */
export const SHARPEN_DEFAULTS: AgSharpen = {
  capture: true,
  captureAmount: 100,
  autoRadius: true,
  radius: 0.75,
  cornerBoost: 0,
  iterations: 20,
  iterCheck: true,
  amount: 0,
  usmRadius: 2,
  threshold: 0.5,
  autoContrast: true,
  contrast: 10,
};

/** `mods/clipping.rs` SHARPEN_MASK. */
export const SHARPEN_MASK = 7;

export function readSharpen(adjustments: any): AgSharpen {
  return { ...SHARPEN_DEFAULTS, ...(adjustments?.agSharpen ?? {}) };
}

/**
 * The mask view: held while Ctrl is down during a drag of one of our
 * sliders, as Lightroom holds it on Alt, or pinned on with the eye.
 *
 * Shown through `previewOverride`, which is theirs and is exactly a render
 * that is never saved - the same way the Ctrl-drag clipping view works (see
 * useThresholdPreview.ts). The override is a copy of the adjustments, so it
 * is rewritten whenever they change, or the mask would freeze on the values
 * it was opened with.
 */
export function useSharpenMask() {
  const [pinned, setPinnedState] = useState(false);
  const pinnedRef = useRef(false);
  const dragging = useRef(false);
  const ctrl = useRef(false);
  const showing = useRef(false);
  const path = useEditorStore((s: any) => s.selectedImage?.path);

  const sync = useCallback(() => {
    const state = useEditorStore.getState() as any;
    const want = pinnedRef.current || (dragging.current && ctrl.current);
    if (want) {
      // Never on top of somebody else's override: before/after uses it too.
      if (!showing.current && state.previewOverride) {
        return;
      }
      showing.current = true;
      state.setEditor({ previewOverride: { ...state.adjustments, showClipping: SHARPEN_MASK } });
    } else if (showing.current) {
      showing.current = false;
      state.setEditor({ previewOverride: null });
    }
  }, []);

  const setPinned = useCallback(
    (on: boolean) => {
      pinnedRef.current = on;
      setPinnedState(on);
      sync();
    },
    [sync],
  );

  // A different photo opens on the photo, not on the mask.
  useEffect(() => {
    pinnedRef.current = false;
    setPinnedState(false);
    sync();
  }, [path, sync]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      ctrl.current = e.ctrlKey || e.metaKey;
      sync();
    };
    const onBlur = () => {
      ctrl.current = false;
      dragging.current = false;
      sync();
    };
    const unsubscribe = useEditorStore.subscribe((s: any, prev: any) => {
      if (showing.current && s.adjustments !== prev.adjustments) {
        s.setEditor({ previewOverride: { ...s.adjustments, showClipping: SHARPEN_MASK } });
      }
    });
    window.addEventListener('keydown', onKey, true);
    window.addEventListener('keyup', onKey, true);
    window.addEventListener('blur', onBlur);
    return () => {
      window.removeEventListener('keydown', onKey, true);
      window.removeEventListener('keyup', onKey, true);
      window.removeEventListener('blur', onBlur);
      unsubscribe();
      // Leaving the panel takes the mask with it.
      if (showing.current) {
        showing.current = false;
        (useEditorStore.getState() as any).setEditor({ previewOverride: null });
      }
    };
  }, [sync]);

  const onDragStateChange = useCallback(
    (isDragging: boolean) => {
      dragging.current = isDragging;
      sync();
    },
    [sync],
  );

  return { pinned, setPinned, onDragStateChange };
}
