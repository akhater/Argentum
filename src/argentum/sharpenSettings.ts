/**
 * Sharpening's settings and its mask views. Ours.
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
  /** The card's own switch: everything on or off. */
  enabled: boolean;
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
  /** RawTherapee's contrast threshold for capture's mask, 0..200. */
  contrast: number;
  /** The manual sharpen uses a mask of its own instead of capture's. */
  usmOwnMask: boolean;
  usmAutoContrast: boolean;
  usmContrast: number;
}

/** RawTherapee's capture sharpening defaults and darktable's sharpen preset. */
export const SHARPEN_DEFAULTS: AgSharpen = {
  enabled: true,
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
  usmOwnMask: false,
  usmAutoContrast: true,
  usmContrast: 10,
};

/** `mods/clipping.rs`: SHARPEN_MASK (capture's) and SHARPEN_MASK_USM. */
export const CAPTURE_MASK = 7;
export const SHARPEN_MASK = 8;
export type MaskMode = typeof CAPTURE_MASK | typeof SHARPEN_MASK;

export function readSharpen(adjustments: any): AgSharpen {
  return { ...SHARPEN_DEFAULTS, ...(adjustments?.agSharpen ?? {}) };
}

/**
 * The mask views: one per group, pinned on with that group's eye, or held
 * while Ctrl is down during a drag of one of that group's sliders, as
 * Lightroom holds it on Alt.
 *
 * Shown through `previewOverride`, which is theirs and is exactly a render
 * that is never saved - the same way the Ctrl-drag clipping view works (see
 * useThresholdPreview.ts). The override is a copy of the adjustments, so it
 * is rewritten whenever they change, or the mask would freeze on the values
 * it was opened with.
 */
export function useSharpenMask() {
  const [pinned, setPinnedState] = useState<MaskMode | null>(null);
  const pinnedRef = useRef<MaskMode | null>(null);
  const dragging = useRef<MaskMode | null>(null);
  const ctrl = useRef(false);
  const showing = useRef<MaskMode | null>(null);
  const path = useEditorStore((s: any) => s.selectedImage?.path);

  const sync = useCallback(() => {
    const state = useEditorStore.getState() as any;
    const want = pinnedRef.current ?? (ctrl.current ? dragging.current : null);
    if (want !== null) {
      // Never on top of somebody else's override: before/after uses it too.
      if (showing.current === null && state.previewOverride) {
        return;
      }
      showing.current = want;
      state.setEditor({ previewOverride: { ...state.adjustments, showClipping: want } });
    } else if (showing.current !== null) {
      showing.current = null;
      state.setEditor({ previewOverride: null });
    }
  }, []);

  /** The eye: show this mask, or stop showing it. */
  const toggle = useCallback(
    (mode: MaskMode) => {
      const next = pinnedRef.current === mode ? null : mode;
      pinnedRef.current = next;
      setPinnedState(next);
      sync();
    },
    [sync],
  );

  /** Stop showing any mask. */
  const clear = useCallback(() => {
    pinnedRef.current = null;
    setPinnedState(null);
    sync();
  }, [sync]);

  // A different photo opens on the photo, not on a mask.
  useEffect(() => {
    pinnedRef.current = null;
    setPinnedState(null);
    sync();
  }, [path, sync]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      ctrl.current = e.ctrlKey || e.metaKey;
      sync();
    };
    const onBlur = () => {
      ctrl.current = false;
      dragging.current = null;
      sync();
    };
    const unsubscribe = useEditorStore.subscribe((s: any, prev: any) => {
      if (showing.current !== null && s.adjustments !== prev.adjustments) {
        s.setEditor({ previewOverride: { ...s.adjustments, showClipping: showing.current } });
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
      if (showing.current !== null) {
        showing.current = null;
        (useEditorStore.getState() as any).setEditor({ previewOverride: null });
      }
    };
  }, [sync]);

  /** For a slider in the group whose mask Ctrl should show. */
  const dragOf = useCallback(
    (mode: MaskMode) => (isDragging: boolean) => {
      dragging.current = isDragging ? mode : null;
      sync();
    },
    [sync],
  );

  return { pinned, toggle, clear, dragOf };
}
