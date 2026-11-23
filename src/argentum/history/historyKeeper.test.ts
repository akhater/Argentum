import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useEditorStore } from '../../store/useEditorStore';
import { INITIAL_ADJUSTMENTS, type Adjustments } from '../../utils/adjustments';
import type { SelectedImage } from '../../components/ui/AppProperties';
import type { HistoryRecord } from './historyRecord';

/** Each photo's sidecar History, as the Rust side keeps it. */
const sidecars = new Map<string, HistoryRecord>();
const writes: string[] = [];

vi.mock('../ag', () => ({
  ag: vi.fn(async (name: string, args: { path: string; record?: HistoryRecord | null }) => {
    if (name === 'history') return JSON.parse(JSON.stringify(sidecars.get(args.path) ?? null));
    if (name === 'set_history') {
      writes.push(args.path);
      if (args.record) sidecars.set(args.path, JSON.parse(JSON.stringify(args.record)));
      else sidecars.delete(args.path);
    }
    return null;
  }),
}));

import { HISTORY_WRITE_AFTER_MS } from '../editTimings';
// Loads the keeper, which watches the editor from its import.
await import('./historyKeeper');

const editor = () => useEditorStore.getState();
const settle = () => vi.advanceTimersByTimeAsync(0);

/** What their code does when a photo opens: the edit read from its sidecar, a History of one state. */
const open = async (path: string, adjustments: Adjustments) => {
  useEditorStore.setState({ selectedImage: { path } as SelectedImage });
  editor().setEditor({ adjustments });
  editor().resetHistory(adjustments);
  await settle();
};

/** A change, as their setAdjustments makes it, and the step their debounce records. */
const change = (values: Partial<Adjustments>, asStep = true) => {
  const adjustments = { ...editor().adjustments, ...values };
  editor().setEditor({ adjustments });
  if (asStep) editor().pushHistory(adjustments);
};

/** The edit as the sidecar gives it back. */
const savedEdit = () => JSON.parse(JSON.stringify(editor().adjustments));

describe('each photo keeps its History', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    sidecars.clear();
    writes.length = 0;
  });
  afterEach(async () => {
    useEditorStore.setState({ selectedImage: null });
    await settle();
    vi.useRealTimers();
  });

  it('comes back after another photo, at the step it was at, and redo still works', async () => {
    await open('a.raf', INITIAL_ADJUSTMENTS);
    change({ exposure: 0.5 });
    change({ contrast: 20 });
    change({ shadows: 30 });
    editor().undo();
    const editA = savedEdit();

    await open('b.raf', INITIAL_ADJUSTMENTS);
    expect(editor().history).toHaveLength(1);

    await open('a.raf', editA);
    expect(editor().history).toHaveLength(4);
    expect(editor().historyIndex).toBe(2);
    expect(editor().adjustments).toEqual(editA);
    editor().redo();
    expect(editor().adjustments.shadows).toBe(30);
    editor().undo();
    editor().undo();
    expect(editor().adjustments.contrast).toBe(INITIAL_ADJUSTMENTS.contrast);
  });

  it('is written once the editor has saved the edit and made its step, not on every change', async () => {
    await open('a.raf', INITIAL_ADJUSTMENTS);
    change({ exposure: 0.5 });
    await vi.advanceTimersByTimeAsync(HISTORY_WRITE_AFTER_MS - 100);
    change({ exposure: 0.6 });
    await vi.advanceTimersByTimeAsync(HISTORY_WRITE_AFTER_MS - 100);
    expect(writes).toEqual([]);
    await vi.advanceTimersByTimeAsync(100);
    expect(writes).toEqual(['a.raf']);
    expect(sidecars.get('a.raf')!.steps).toHaveLength(3);
  });

  it('keeps a change not yet a step when the photo is left, as the step it would have been', async () => {
    await open('a.raf', INITIAL_ADJUSTMENTS);
    change({ exposure: 0.5 });
    change({ contrast: 20 }, false);
    const editA = savedEdit();

    await open('b.raf', INITIAL_ADJUSTMENTS);
    await open('a.raf', editA);
    expect(editor().history).toHaveLength(3);
    expect(editor().history[2]).toBe(editor().adjustments);
    editor().undo();
    expect(editor().adjustments.contrast).toBe(INITIAL_ADJUSTMENTS.contrast);
  });

  it('starts fresh when the edit has changed since', async () => {
    await open('a.raf', INITIAL_ADJUSTMENTS);
    change({ exposure: 0.5 });
    const pasted = { ...savedEdit(), exposure: 1.5 };

    await open('b.raf', INITIAL_ADJUSTMENTS);
    await open('a.raf', pasted);
    expect(editor().history).toEqual([pasted]);
    expect(editor().adjustments).toBe(pasted);
  });

  it('writes nothing for a photo opened and left unedited', async () => {
    await open('a.raf', INITIAL_ADJUSTMENTS);
    await vi.advanceTimersByTimeAsync(HISTORY_WRITE_AFTER_MS * 2);
    await open('b.raf', INITIAL_ADJUSTMENTS);
    await vi.advanceTimersByTimeAsync(HISTORY_WRITE_AFTER_MS * 2);
    expect(writes).toEqual([]);
  });

  it('shows each step under the name it was saved with', async () => {
    await open('a.raf', INITIAL_ADJUSTMENTS);
    change({ exposure: 0.5 });
    change({ vibrance: 10 });
    const editA = savedEdit();
    await open('b.raf', INITIAL_ADJUSTMENTS);
    change({ contrast: 5 });
    change({ shadows: 5 });

    await open('a.raf', editA);
    const { historyNames } = await import('../layout/historyNames');
    expect(historyNames(editor().history)).toEqual(['Initial State', 'Exposure', 'Vibrance']);
  });
});
