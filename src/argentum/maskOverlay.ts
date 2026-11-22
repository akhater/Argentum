/**
 * The selected mask's red overlay: shown, turned off with O, or hidden by adjusting.
 *
 * RAWmakase d2bafbff: src/app/develop/tools/mask_tool.rs (MaskTool::overlay,
 * hidden_by_adjusting). As in Lightroom, moving one of a mask's adjustment
 * sliders (or resetting them) hides its overlay, so what they do shows; O
 * brings it back, as does selecting or making another mask or component. An
 * overlay turned off with O stays off for the next mask.
 *
 * Their overlay was already hidden while a slider is dragged or the pointer is
 * over the mask's sliders (ImageCanvas's maskOpacity); this keeps it hidden
 * afterwards. Masks only: the AI panel's patches keep their own rules.
 */

import { create } from 'zustand';
import { useEditorStore } from '../store/useEditorStore';

export interface MaskOverlay {
  shown: boolean;
  /** Whether adjusting hid it, rather than O: it shows again for the next mask. */
  hiddenByAdjusting: boolean;
}

export const OVERLAY_SHOWN: MaskOverlay = { shown: true, hiddenByAdjusting: false };

/** O: on or off, the user's own choice. */
export const toggled = (o: MaskOverlay): MaskOverlay => ({ shown: !o.shown, hiddenByAdjusting: false });

/** A mask's adjustment changed: hidden, unless already off. */
export const afterAdjusting = (o: MaskOverlay): MaskOverlay =>
  o.shown ? { shown: false, hiddenByAdjusting: true } : o;

/** Another mask or component selected or made (or none): shown again if adjusting hid it. */
export const afterSelecting = (o: MaskOverlay): MaskOverlay => (o.hiddenByAdjusting ? OVERLAY_SHOWN : o);

interface MaskOverlayState extends MaskOverlay {
  toggle(): void;
  adjusted(): void;
}

export const useMaskOverlay = create<MaskOverlayState>((set) => ({
  ...OVERLAY_SHOWN,
  toggle: () => set(toggled),
  adjusted: () => set(afterAdjusting),
}));

// A component too: theirs shows a selection just made, and ours selects a component as it is added.
useEditorStore.subscribe((state, prev) => {
  if (state.activeMaskContainerId !== prev.activeMaskContainerId || state.activeMaskId !== prev.activeMaskId) {
    useMaskOverlay.setState(afterSelecting);
  }
});
