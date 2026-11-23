/* eslint-disable @typescript-eslint/no-explicit-any */
// More answers that arrive after the user opened another photo (see lateResults.test.ts): the metadata
// refresh, the lens pick, AI results and the pending History step. Each runs with the backend held back,
// switches the open photo, lets the answer through, and the new photo must be as it was. Staying put,
// or an answer for a photo no longer open, must still end up on the photo it belongs to.
import { createElement } from 'react';
import { renderToString } from 'react-dom/server';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const backend = vi.hoisted(() => ({
  held: new Map<string, { promise: Promise<unknown>; release: (value: unknown) => void; fail: (e: unknown) => void }>(),
  answers: new Map<string, unknown>(),
  calls: [] as Array<{ command: string; args: any }>,
}));

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (command: string, args: any) => {
    backend.calls.push({ command, args });
    const key = command === 'ag' ? `ag:${args.name}` : command;
    if (key === 'ag:selection_models_status') return Promise.resolve({ installed: true });
    return backend.held.get(key)?.promise ?? Promise.resolve(backend.answers.get(key));
  },
}));
vi.mock('@tauri-apps/api/event', () => ({ listen: () => Promise.resolve(() => {}) }));
vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock('react-toastify', () => ({ toast: { error: vi.fn(), info: vi.fn(), update: vi.fn() } }));

import { useAiMasking } from '../hooks/useAiMasking';
import { debouncedSave, useEditorActions } from '../hooks/useEditorActions';
import { debouncedSetHistory } from './historyOnRelease';
import { putLensOn } from './lensPick';
import { refreshMetadataFor } from './RefreshMetadataButton';
import { useSelectionMasks } from './selection/selection';
import { useEditorStore } from '../store/useEditorStore';
import { useLibraryStore } from '../store/useLibraryStore';
import { Invokes, type SelectedImage } from '../components/ui/AppProperties';
import { INITIAL_ADJUSTMENTS, type Adjustments } from '../utils/adjustments';

function hook<T>(use: () => T): T {
  let result!: T;
  renderToString(
    createElement(() => {
      result = use();
      return null;
    }),
  );
  return result;
}

/** The backend's answer to `key` waits until `release` (or `fail`) is called. */
function hold(key: string) {
  let release!: (value: unknown) => void;
  let fail!: (e: unknown) => void;
  const promise = new Promise<unknown>((resolve, reject) => {
    release = resolve;
    fail = reject;
  });
  backend.held.set(key, { promise, release, fail });
  return { release: (value?: unknown) => release(value), fail: (e: unknown) => fail(e) };
}

const patch = {
  id: 'p1',
  name: 'Patch',
  visible: true,
  invert: false,
  prompt: '',
  isLoading: false,
  subMasks: [] as unknown[],
  patchData: null,
};
const mask = { id: 'm1', name: 'Mask', visible: true, subMasks: [{ id: 's1', type: 'ai-subject', parameters: {} }] };
const edit = (extra: Partial<Adjustments>): Adjustments => ({ ...INITIAL_ADJUSTMENTS, ...extra });
const editA = edit({
  exposure: 1,
  lensMaker: 'MakerA',
  lensModel: 'LensA',
  aiPatches: [patch as never],
  masks: [mask] as never,
});
const editB = edit({ exposure: -1, lensMaker: 'MakerB', lensModel: 'LensB' });
const historyB = [INITIAL_ADJUSTMENTS, editB];
const imageA = { path: '/p/a.CR2', isReady: true, exif: { Model: 'old A' } } as unknown as SelectedImage;
const imageB = { path: '/p/b.CR2', isReady: true, exif: { Model: 'B' } } as unknown as SelectedImage;

function open(image: SelectedImage, adjustments: Adjustments, history: Adjustments[]) {
  useEditorStore.setState({
    selectedImage: image,
    adjustments,
    history,
    historyIndex: history.length - 1,
  });
}
const openA = () => open(imageA, editA, [INITIAL_ADJUSTMENTS, editA]);
const switchToB = () => open(imageB, editB, historyB);

function expectBUntouched() {
  const state = useEditorStore.getState();
  expect(state.selectedImage).toBe(imageB);
  expect(state.adjustments).toBe(editB);
  expect(state.history).toBe(historyB);
  expect(state.historyIndex).toBe(1);
}

const settle = () => new Promise((resolve) => setTimeout(resolve, 0));
const savesFor = (path: string) =>
  backend.calls.filter((c) => c.command === Invokes.SaveMetadataAndUpdateThumbnail && c.args.path === path);

describe('answers that arrive after another photo was opened', () => {
  beforeEach(() => {
    backend.held.clear();
    backend.answers.clear();
    backend.calls.length = 0;
    debouncedSetHistory.cancel();
    debouncedSave.cancel();
    useLibraryStore.setState({ multiSelectedPaths: [], libraryActivePath: null });
  });

  describe('Re-read metadata', () => {
    it("A's EXIF does not land on B", async () => {
      openA();
      const answer = hold('ag:refresh_image_metadata');
      const running = refreshMetadataFor('/p/a.CR2');
      switchToB();
      answer.release({ Model: 'new A' });
      await running;
      expectBUntouched();
    });

    it('staying on A puts the fresh EXIF on A', async () => {
      openA();
      const answer = hold('ag:refresh_image_metadata');
      const running = refreshMetadataFor('/p/a.CR2');
      answer.release({ Model: 'new A' });
      await running;
      expect(useEditorStore.getState().selectedImage?.exif).toEqual({ Model: 'new A' });
      expect(useEditorStore.getState().selectedImage?.path).toBe('/p/a.CR2');
    });
  });

  describe('Lens pick (Crop panel)', () => {
    const params = { k1: 0.1 };
    it("A's lens does not land on B", async () => {
      openA();
      const answer = hold('get_lens_distortion_params');
      const { setAdjustments } = hook(useEditorActions);
      const running = putLensOn('/p/a.CR2', {}, 'NewMaker', 'NewLens', setAdjustments);
      switchToB();
      answer.release(params);
      await running;
      debouncedSetHistory.flush();
      expectBUntouched();
    });

    it('staying on A puts the lens on A', async () => {
      openA();
      const answer = hold('get_lens_distortion_params');
      const { setAdjustments } = hook(useEditorActions);
      const running = putLensOn('/p/a.CR2', {}, 'NewMaker', 'NewLens', setAdjustments, { lensCorrectionMode: 'auto' });
      answer.release(params);
      expect(await running).toBe(true);
      const now = useEditorStore.getState().adjustments;
      expect(now.lensMaker).toBe('NewMaker');
      expect(now.lensModel).toBe('NewLens');
      expect(now.lensDistortionParams).toEqual(params);
      expect(now.lensCorrectionMode).toBe('auto');
    });
  });

  describe('Generative replace', () => {
    const result = JSON.stringify({ png: 'x' });
    const savedOnDisk = () => ({
      adjustments: { aiPatches: [{ ...patch, isLoading: true, subMasks: [] }] },
    });

    it("A's result does not land on B, and A's saved edit gets it", async () => {
      openA();
      backend.answers.set(Invokes.LoadMetadata, savedOnDisk());
      const answer = hold(Invokes.InvokeGenerativeReplaseWithMaskDef);
      const { handleGenerativeReplace } = hook(useAiMasking);
      const running = handleGenerativeReplace('p1', 'a cat', false);
      switchToB();
      answer.release(result);
      await running;
      await settle();
      debouncedSetHistory.flush();
      expectBUntouched();
      const saved = savesFor('/p/a.CR2');
      expect(saved).toHaveLength(1);
      const savedPatch = saved[0].args.adjustments.aiPatches[0];
      expect(savedPatch.isLoading).toBe(false);
      expect(savedPatch.patchData).toEqual({ png: 'x' });
    });

    it("a failure for A clears A's loading flag on disk and leaves B alone", async () => {
      openA();
      backend.answers.set(Invokes.LoadMetadata, savedOnDisk());
      const answer = hold(Invokes.InvokeGenerativeReplaseWithMaskDef);
      const { handleGenerativeReplace } = hook(useAiMasking);
      const running = handleGenerativeReplace('p1', 'a cat', false);
      switchToB();
      answer.fail('cancelled');
      await running;
      await settle();
      debouncedSetHistory.flush();
      expectBUntouched();
      expect(savesFor('/p/a.CR2')[0].args.adjustments.aiPatches[0].isLoading).toBe(false);
    });

    it('staying on A applies the result to A and clears the flag', async () => {
      openA();
      const answer = hold(Invokes.InvokeGenerativeReplaseWithMaskDef);
      const { handleGenerativeReplace } = hook(useAiMasking);
      const running = handleGenerativeReplace('p1', 'a cat', false);
      expect(useEditorStore.getState().adjustments.aiPatches[0].isLoading).toBe(true);
      answer.release(result);
      await running;
      const now = useEditorStore.getState().adjustments.aiPatches[0];
      expect(now.isLoading).toBe(false);
      expect(now.patchData).toEqual({ png: 'x' });
      expect(savesFor('/p/a.CR2')).toHaveLength(0);
    });
  });

  describe('AI mask', () => {
    it("A's mask does not land on B, and A's saved edit gets it", async () => {
      openA();
      backend.answers.set(Invokes.LoadMetadata, { adjustments: { masks: [mask], aiPatches: [] } });
      const answer = hold(Invokes.GenerateAiSubjectMask);
      const { handleGenerateAiMask } = hook(useAiMasking);
      const running = handleGenerateAiMask('s1', { x: 0, y: 0 }, { x: 1, y: 1 });
      switchToB();
      answer.release({ maskDataBase64: 'abc' });
      await running;
      await settle();
      debouncedSetHistory.flush();
      expectBUntouched();
      const saved = savesFor('/p/a.CR2');
      expect(saved).toHaveLength(1);
      expect(saved[0].args.adjustments.masks[0].subMasks[0].parameters.maskDataBase64).toBe('abc');
    });

    it('staying on A puts the mask on A', async () => {
      openA();
      const answer = hold(Invokes.GenerateAiSubjectMask);
      const { handleGenerateAiMask } = hook(useAiMasking);
      const running = handleGenerateAiMask('s1', { x: 0, y: 0 }, { x: 1, y: 1 });
      answer.release({ maskDataBase64: 'abc' });
      await running;
      const sub = useEditorStore.getState().adjustments.masks[0].subMasks[0] as any;
      expect(sub.parameters.maskDataBase64).toBe('abc');
    });
  });

  describe('Sky / Subject selection', () => {
    it("A's selection does not land on B, and A's saved edit gets it", async () => {
      openA();
      backend.answers.set(Invokes.LoadMetadata, { adjustments: { masks: [mask], aiPatches: [] } });
      const answer = hold('ag:selection_mask');
      const make = hook(useSelectionMasks);
      const running = make('s1', 'sky');
      await settle();
      switchToB();
      answer.release({ found: true, maskDataBase64: 'sky' });
      await running;
      await settle();
      debouncedSetHistory.flush();
      expectBUntouched();
      const saved = savesFor('/p/a.CR2');
      expect(saved).toHaveLength(1);
      expect(saved[0].args.adjustments.masks[0].subMasks[0].parameters.maskDataBase64).toBe('sky');
    });

    it('staying on A fills the selection', async () => {
      openA();
      const answer = hold('ag:selection_mask');
      const make = hook(useSelectionMasks);
      const running = make('s1', 'sky');
      await settle();
      answer.release({ found: true, maskDataBase64: 'sky' });
      await running;
      const sub = useEditorStore.getState().adjustments.masks[0].subMasks[0] as any;
      expect(sub.parameters.maskDataBase64).toBe('sky');
    });
  });

  describe('the pending History step', () => {
    it("a step queued on A is not pushed onto B's history", () => {
      openA();
      debouncedSetHistory(edit({ exposure: 3 }));
      switchToB();
      debouncedSetHistory.flush();
      expectBUntouched();
    });

    it('a step queued on A and flushed on A is recorded', () => {
      openA();
      const next = edit({ exposure: 3 });
      debouncedSetHistory(next);
      debouncedSetHistory.flush();
      expect(useEditorStore.getState().history.at(-1)).toBe(next);
    });
  });
});
