import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useEditorStore } from '../store/useEditorStore';
import { HISTORY_STEP_AFTER_MS } from './editTimings';
import { debouncedSetHistory } from './historyOnRelease';
import { deleteSelectedContainer } from './deleteSelected';
import { INITIAL_ADJUSTMENTS, type Adjustments, type AiPatch, type MaskContainer } from '../utils/adjustments';
import type { SelectedImage } from '../components/ui/AppProperties';

/** What their setAdjustments does on every change. */
const setAdjustments = (change: (prev: Adjustments) => Adjustments) => {
  const adjustments = change(useEditorStore.getState().adjustments);
  useEditorStore.setState({ adjustments });
  debouncedSetHistory(adjustments);
};

const withContainers: Adjustments = {
  ...INITIAL_ADJUSTMENTS,
  masks: [{ id: 'm1' } as MaskContainer, { id: 'm2' } as MaskContainer],
  aiPatches: [{ id: 'p1' } as AiPatch],
};

describe('Delete on a selected mask or AI patch', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    useEditorStore.setState({ selectedImage: { path: 'a.raf' } as SelectedImage, isSliderDragging: false });
    useEditorStore.getState().resetHistory(withContainers);
  });
  afterEach(() => {
    debouncedSetHistory.cancel();
    vi.useRealTimers();
  });

  it('is a History step that Undo puts back', () => {
    useEditorStore.setState({ activeMaskContainerId: 'm1', activeMaskId: 'x' });
    deleteSelectedContainer(setAdjustments);
    const state = useEditorStore.getState();
    expect(state.adjustments.masks.map((m) => m.id)).toEqual(['m2']);
    expect([state.activeMaskContainerId, state.activeMaskId]).toEqual([null, null]);
    vi.advanceTimersByTime(HISTORY_STEP_AFTER_MS);
    expect(useEditorStore.getState().history.length).toBe(2);
    useEditorStore.getState().undo();
    expect(useEditorStore.getState().adjustments.masks.map((m) => m.id)).toEqual(['m1', 'm2']);
  });

  it('deletes an AI patch the same way', () => {
    useEditorStore.setState({ activeMaskContainerId: null, activeAiPatchContainerId: 'p1', activeAiSubMaskId: 's' });
    deleteSelectedContainer(setAdjustments);
    expect(useEditorStore.getState().adjustments.aiPatches).toEqual([]);
    expect(useEditorStore.getState().activeAiPatchContainerId).toBeNull();
    useEditorStore.getState().undo();
    expect(useEditorStore.getState().adjustments.aiPatches.map((p) => p.id)).toEqual(['p1']);
  });
});
