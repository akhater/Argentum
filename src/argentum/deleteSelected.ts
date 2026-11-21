/**
 * Delete (Backspace on macOS) on the selected mask or AI patch. Ours.
 *
 * Their shortcut set the photo's settings straight on the store, so the deletion
 * was no History step: Undo went past it. It goes through setAdjustments here,
 * as any other change does, and becomes a step.
 */

import { useEditorStore } from '../store/useEditorStore';
import type { Adjustments } from '../utils/adjustments';

/** Their setAdjustments (hooks/useEditorActions.ts): changes the photo's settings and records the step. */
type SetAdjustments = (change: (prev: Adjustments) => Adjustments) => void;

// RapidRAW 5720d633: src/hooks/useKeyboardShortcuts.ts (mask and AI patch deletion through setAdjustments)
export function deleteSelectedContainer(setAdjustments: SetAdjustments): void {
  const { activeMaskContainerId, activeAiPatchContainerId, setEditor } = useEditorStore.getState();
  if (activeMaskContainerId) {
    setAdjustments((prev) => ({ ...prev, masks: prev.masks.filter((c) => c.id !== activeMaskContainerId) }));
    setEditor({ activeMaskContainerId: null, activeMaskId: null });
  } else if (activeAiPatchContainerId) {
    setAdjustments((prev) => ({
      ...prev,
      aiPatches: prev.aiPatches.filter((c) => c.id !== activeAiPatchContainerId),
    }));
    setEditor({ activeAiPatchContainerId: null, activeAiSubMaskId: null });
  }
}
