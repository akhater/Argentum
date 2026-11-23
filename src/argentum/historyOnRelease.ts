/**
 * When a change becomes a History step. Ours.
 *
 * Their editor records a step HISTORY_STEP_AFTER_MS after the last change (debouncedSetHistory,
 * which lived in src/hooks/useEditorActions.ts), whether or not the mouse is
 * still down. So a drag that paused now and then left a step at every pause:
 * eight "LUT Intensity" steps for one drag.
 *
 * While isSliderDragging is on, the step they ask for is held here instead, and
 * letting go records the latest one at once. Their sliders, curves, colour
 * wheels and the crop's pan and wheel-zoom all set that flag, so each of them
 * becomes one step. A change made without a drag (a click, a typed value, a
 * double-click reset) keeps their HISTORY_STEP_AFTER_MS.
 *
 * Undo, redo and a jump in the History record a change still waiting in that
 * delay first, so Undo right after a click or a typed value undoes it rather
 * than the step before, and the first change of a photo can be undone at once.
 *
 * Installed at import by replacing the store's pushHistory, undo, redo and
 * goToHistoryIndex, so no line of theirs changes.
 */

import debounce from 'lodash.debounce';
import { useEditorStore } from '../store/useEditorStore';
import type { Adjustments } from '../utils/adjustments';
import { HISTORY_STEP_AFTER_MS } from './editTimings';

const { pushHistory: recordStep, undo, redo, goToHistoryIndex } = useEditorStore.getState();

/** Whether a change is waiting in the HISTORY_STEP_AFTER_MS delay to become a step. */
let pending = false;

// The step carries the photo it was asked on: after a switch it is dropped, not pushed onto the new photo.
const stepSoon = debounce((adjustments: Adjustments, path: string | undefined) => {
  pending = false;
  if (useEditorStore.getState().selectedImage?.path !== path) return;
  useEditorStore.getState().pushHistory(adjustments);
}, HISTORY_STEP_AFTER_MS);

/** Their step after a change (HISTORY_STEP_AFTER_MS), moved here from useEditorActions.ts so this module knows one is waiting. */
// RapidRAW 5720d633: src/store/useEditorStore.ts (the debounce moved where undo can flush it)
export const debouncedSetHistory = Object.assign(
  (adjustments: Adjustments) => {
    pending = true;
    stepSoon(adjustments, useEditorStore.getState().selectedImage?.path);
  },
  {
    flush: () => stepSoon.flush(),
    cancel: () => {
      pending = false;
      stepSoon.cancel();
    },
  },
);

/** Whether Undo has something to undo: a step behind, or a change not yet a step. */
// RapidRAW 5720d633: src/hooks/useKeyboardShortcuts.ts, useAppContextMenus.ts (undo enabled for a pending edit)
export const canUndo = (state: { historyIndex: number }): boolean => state.historyIndex > 0 || pending;

/** The latest step asked for during the drag, and the photo it belongs to. */
let held: { path: string | undefined; adjustments: Adjustments } | null = null;

// RapidRAW 5720d633: src/store/useEditorStore.ts (undo, redo and history jumps flush the pending step)
const afterPendingStep =
  <A extends unknown[]>(move: (...args: A) => void) =>
  (...args: A) => {
    debouncedSetHistory.flush();
    move(...args);
  };

useEditorStore.setState({
  pushHistory: (adjustments: Adjustments) => {
    const { isSliderDragging, selectedImage } = useEditorStore.getState();
    if (isSliderDragging) {
      held = { path: selectedImage?.path, adjustments };
      return;
    }
    held = null;
    recordStep(adjustments);
  },
  undo: afterPendingStep(undo),
  redo: afterPendingStep(redo),
  goToHistoryIndex: afterPendingStep(goToHistoryIndex),
});

useEditorStore.subscribe((state, prev) => {
  if (!prev.isSliderDragging || state.isSliderDragging) return;
  // A change in the drag's last HISTORY_STEP_AFTER_MS is still waiting in their debounce:
  // recorded now, through the pushHistory above, and newer than anything held.
  debouncedSetHistory.flush();
  // A photo opened mid-drag started a new history; the held step was the old photo's.
  if (held && held.path === state.selectedImage?.path) recordStep(held.adjustments);
  held = null;
});
