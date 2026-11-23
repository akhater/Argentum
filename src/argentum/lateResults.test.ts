// A backend answer that arrives after the user opened another photo belongs to the photo the action
// started on (hooks/useEditorActions.ts). Each handler is run with the backend held back: switch the
// open photo, let the answer through, and the new photo must be as it was. Staying put changes nothing.
import { createElement } from 'react';
import { renderToString } from 'react-dom/server';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const backend = vi.hoisted(() => ({
  held: new Map<string, { promise: Promise<unknown>; release: (value: unknown) => void }>(),
  calls: [] as string[],
}));

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (command: string) => {
    backend.calls.push(command);
    return backend.held.get(command)?.promise ?? Promise.resolve(undefined);
  },
}));

vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock('react-toastify', () => ({ toast: { error: vi.fn(), info: vi.fn() } }));

import { reloadAdjustmentsForPaths, useEditorActions } from '../hooks/useEditorActions';
import { debouncedSetHistory } from './historyOnRelease';
import { useEditorStore } from '../store/useEditorStore';
import { useLibraryStore } from '../store/useLibraryStore';
import { useSettingsStore } from '../store/useSettingsStore';
import { Invokes, type SelectedImage } from '../components/ui/AppProperties';
import { INITIAL_ADJUSTMENTS, PasteMode, type Adjustments } from '../utils/adjustments';

/** The hook's handlers, from one server render (no DOM here). */
function actions() {
  let result!: ReturnType<typeof useEditorActions>;
  renderToString(
    createElement(() => {
      result = useEditorActions();
      return null;
    }),
  );
  return result;
}

/** The backend's answer to `command` waits until the returned function is called. */
function hold(command: string): (value?: unknown) => void {
  let release!: (value: unknown) => void;
  const promise = new Promise<unknown>((resolve) => (release = resolve));
  backend.held.set(command, { promise, release });
  return (value) => release(value);
}

const edit = (extra: Partial<Adjustments>): Adjustments => ({ ...INITIAL_ADJUSTMENTS, ...extra });
const editA = edit({ exposure: 1, lensMaker: 'MakerA', lensModel: 'LensA' });
const editB = edit({ exposure: -1, lensMaker: 'MakerB', lensModel: 'LensB' });
const historyB = [INITIAL_ADJUSTMENTS, editB];

function open(path: string, adjustments: Adjustments, history: Adjustments[]) {
  useEditorStore.setState({
    selectedImage: { path, isReady: true } as SelectedImage,
    adjustments,
    history,
    historyIndex: history.length - 1,
  });
}
const openA = () => open('/p/a.CR2', editA, [INITIAL_ADJUSTMENTS, editA]);
const switchToB = () => open('/p/b.CR2', editB, historyB);

function expectBUntouched() {
  const state = useEditorStore.getState();
  expect(state.selectedImage?.path).toBe('/p/b.CR2');
  expect(state.adjustments).toBe(editB);
  expect(state.history).toBe(historyB);
  expect(state.historyIndex).toBe(1);
}

const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

describe('answers that arrive after another photo was opened', () => {
  beforeEach(() => {
    backend.held.clear();
    backend.calls.length = 0;
    debouncedSetHistory.cancel();
    useLibraryStore.setState({ multiSelectedPaths: [], libraryActivePath: null });
  });

  it("Auto: photo A's values do not land on B", async () => {
    openA();
    const answer = hold(Invokes.CalculateAutoAdjustments);
    const running = actions().handleAutoAdjustments();
    switchToB();
    answer({ exposure: 5 });
    await running;
    debouncedSetHistory.flush();
    expectBUntouched();
  });

  it('Auto: staying on A applies the values', async () => {
    openA();
    const answer = hold(Invokes.CalculateAutoAdjustments);
    const running = actions().handleAutoAdjustments();
    answer({ exposure: 5 });
    await running;
    expect(useEditorStore.getState().adjustments.exposure).toBe(5);
  });

  it('LUT: the LUT picked on A does not land on B', async () => {
    openA();
    const answer = hold('load_and_parse_lut');
    const running = actions().handleLutSelect('/luts/x.cube');
    switchToB();
    answer({ size: 33 });
    await running;
    debouncedSetHistory.flush();
    expectBUntouched();
  });

  it('LUT: staying on A puts the LUT on A', async () => {
    openA();
    const answer = hold('load_and_parse_lut');
    const running = actions().handleLutSelect('/luts/x.cube');
    answer({ size: 33 });
    await running;
    const now = useEditorStore.getState().adjustments;
    expect(now.lutPath).toBe('/luts/x.cube');
    expect(now.lutSize).toBe(33);
  });

  it('Reset: B is not reset, and its undo is kept', async () => {
    openA();
    const answer = hold(Invokes.ResetAdjustmentsForPaths);
    actions().handleResetAdjustments(['/p/a.CR2']);
    switchToB();
    answer();
    await settle();
    expectBUntouched();
    expect(backend.calls).toContain(Invokes.ResetAdjustmentsForPaths);
  });

  it('Reset: staying on A resets A and its undo', async () => {
    openA();
    const answer = hold(Invokes.ResetAdjustmentsForPaths);
    actions().handleResetAdjustments(['/p/a.CR2']);
    answer();
    await settle();
    const state = useEditorStore.getState();
    expect(state.adjustments).toEqual({ ...INITIAL_ADJUSTMENTS, aiPatches: [] });
    expect(state.history).toHaveLength(1);
    expect(state.historyIndex).toBe(0);
  });

  describe('Paste', () => {
    beforeEach(() => {
      useSettingsStore.setState({
        appSettings: { copyPasteSettings: { mode: PasteMode.Replace, includedAdjustments: ['exposure'] } } as never,
      });
      useEditorStore.setState({ copiedAdjustments: { exposure: 2 } as Partial<Adjustments> as never });
    });
    const metaOfA = { adjustments: { lensMaker: 'PastedMaker', lensModel: 'PastedLens', lensDistortionParams: null } };

    it("A's lens does not land on B", async () => {
      openA();
      const applied = hold(Invokes.ApplyAdjustmentsToPaths);
      const meta = hold(Invokes.LoadMetadata);
      actions().handlePasteAdjustments(['/p/a.CR2']);
      debouncedSetHistory.cancel();
      applied();
      await settle();
      switchToB();
      meta(metaOfA);
      await settle();
      debouncedSetHistory.flush();
      expectBUntouched();
    });

    it("staying on A puts the pasted photo's lens fields on A", async () => {
      openA();
      const applied = hold(Invokes.ApplyAdjustmentsToPaths);
      const meta = hold(Invokes.LoadMetadata);
      actions().handlePasteAdjustments(['/p/a.CR2']);
      applied();
      await settle();
      meta(metaOfA);
      await settle();
      const now = useEditorStore.getState().adjustments;
      expect(now.exposure).toBe(2);
      expect(now.lensMaker).toBe('PastedMaker');
      expect(now.lensModel).toBe('PastedLens');
    });
  });

  it("a batch reload does not write A's saved edit over B", async () => {
    openA();
    const meta = hold(Invokes.LoadMetadata);
    const reloading = reloadAdjustmentsForPaths(['/p/a.CR2']);
    switchToB();
    meta({ adjustments: { exposure: 9 } });
    await reloading;
    expectBUntouched();
  });

  it('a batch reload on the open photo replaces its edit from disk', async () => {
    openA();
    const meta = hold(Invokes.LoadMetadata);
    const reloading = reloadAdjustmentsForPaths(['/p/a.CR2']);
    meta({ adjustments: { exposure: 9 } });
    await reloading;
    const state = useEditorStore.getState();
    expect(state.adjustments.exposure).toBe(9);
    expect(state.history).toHaveLength(1);
  });
});
