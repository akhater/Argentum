import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useEditorStore } from '../store/useEditorStore';
import { HISTORY_STEP_AFTER_MS } from './editTimings';
import { debouncedSetHistory } from '../hooks/useEditorActions';
import { INITIAL_ADJUSTMENTS, type Adjustments } from '../utils/adjustments';
import type { SelectedImage } from '../components/ui/AppProperties';
import { canUndo } from './historyOnRelease';

const photo = (path: string) => ({ path }) as SelectedImage;
const withExposure = (exposure: number): Adjustments => ({ ...INITIAL_ADJUSTMENTS, exposure });
const steps = () => useEditorStore.getState().history.length - 1;

/** What their setAdjustments does on every change. */
const change = (exposure: number) => {
  const adjustments = withExposure(exposure);
  useEditorStore.setState({ adjustments });
  debouncedSetHistory(adjustments);
};
const drag = (on: boolean) => useEditorStore.setState({ isSliderDragging: on });

describe('history on release', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    useEditorStore.setState({ selectedImage: photo('a.raf'), isSliderDragging: false });
    useEditorStore.getState().resetHistory(INITIAL_ADJUSTMENTS);
  });
  afterEach(() => {
    debouncedSetHistory.cancel();
    vi.useRealTimers();
  });

  it('records nothing while held, however long the pause, and one step on release', () => {
    drag(true);
    change(0.2);
    vi.advanceTimersByTime(2000);
    change(0.5);
    vi.advanceTimersByTime(2000);
    expect(steps()).toBe(0);
    drag(false);
    expect(steps()).toBe(1);
    expect(useEditorStore.getState().history[1].exposure).toBe(0.5);
  });

  it('records a change made just before release at once, and only once', () => {
    drag(true);
    change(0.2);
    vi.advanceTimersByTime(2000);
    change(0.7);
    drag(false);
    vi.advanceTimersByTime(2000);
    expect(steps()).toBe(1);
    expect(useEditorStore.getState().history[1].exposure).toBe(0.7);
  });

  it('records nothing for a drag that changed nothing', () => {
    drag(true);
    vi.advanceTimersByTime(2000);
    drag(false);
    expect(steps()).toBe(0);
  });

  it('keeps their step delay for a change without a drag', () => {
    change(0.3);
    expect(steps()).toBe(0);
    vi.advanceTimersByTime(HISTORY_STEP_AFTER_MS);
    expect(steps()).toBe(1);
  });

  it('drops a step held for a photo that is no longer open', () => {
    drag(true);
    change(0.4);
    vi.advanceTimersByTime(2000);
    useEditorStore.setState({ selectedImage: photo('b.raf') });
    useEditorStore.getState().resetHistory(INITIAL_ADJUSTMENTS);
    drag(false);
    expect(steps()).toBe(0);
  });
});

describe('undo after a change that is not a step yet', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    useEditorStore.setState({ selectedImage: photo('a.raf'), isSliderDragging: false });
    useEditorStore.getState().resetHistory(INITIAL_ADJUSTMENTS);
  });
  afterEach(() => {
    debouncedSetHistory.cancel();
    vi.useRealTimers();
  });

  it('undoes the first change of a photo at once, and redo brings it back', () => {
    change(0.3);
    expect(canUndo(useEditorStore.getState())).toBe(true);
    useEditorStore.getState().undo();
    expect(useEditorStore.getState().adjustments.exposure).toBe(INITIAL_ADJUSTMENTS.exposure);
    vi.advanceTimersByTime(2000);
    expect(useEditorStore.getState().adjustments.exposure).toBe(INITIAL_ADJUSTMENTS.exposure);
    useEditorStore.getState().redo();
    expect(useEditorStore.getState().adjustments.exposure).toBe(0.3);
  });

  it('undoes the latest change, not the step before it', () => {
    change(0.3);
    vi.advanceTimersByTime(HISTORY_STEP_AFTER_MS);
    change(0.6);
    useEditorStore.getState().undo();
    expect(useEditorStore.getState().adjustments.exposure).toBe(0.3);
    expect(useEditorStore.getState().historyIndex).toBe(1);
  });

  it('keeps a pending change when jumping in the History', () => {
    change(0.3);
    useEditorStore.getState().goToHistoryIndex(0);
    expect(steps()).toBe(1);
    expect(useEditorStore.getState().adjustments.exposure).toBe(INITIAL_ADJUSTMENTS.exposure);
  });

  it('has nothing to undo on a fresh photo, nor once a pending change is dropped', () => {
    expect(canUndo(useEditorStore.getState())).toBe(false);
    change(0.3);
    debouncedSetHistory.cancel();
    expect(canUndo(useEditorStore.getState())).toBe(false);
  });
});
