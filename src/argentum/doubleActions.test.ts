/* eslint-disable @typescript-eslint/no-explicit-any */
// A second trigger while the first run is still going does nothing: Generate (same patch) and Paste files.
import { createElement } from 'react';
import { renderToString } from 'react-dom/server';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const backend = vi.hoisted(() => ({
  held: new Map<string, { promise: Promise<unknown>; release: (value: unknown) => void }>(),
  calls: [] as Array<{ command: string; args: any }>,
}));

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (command: string, args: any) => {
    backend.calls.push({ command, args });
    return backend.held.get(command)?.promise ?? Promise.resolve(undefined);
  },
}));
vi.mock('@tauri-apps/api/event', () => ({ listen: () => Promise.resolve(() => {}) }));
vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock('react-toastify', () => ({ toast: { error: vi.fn(), info: vi.fn(), update: vi.fn() } }));

import { useAiMasking } from '../hooks/useAiMasking';
import { useFileOperations } from '../hooks/useFileOperations';
import { debouncedSetHistory } from './historyOnRelease';
import { useEditorStore } from '../store/useEditorStore';
import { useLibraryStore } from '../store/useLibraryStore';
import { useProcessStore } from '../store/useProcessStore';
import { Invokes, type SelectedImage } from '../components/ui/AppProperties';
import { INITIAL_ADJUSTMENTS } from '../utils/adjustments';

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

function hold(key: string) {
  let release!: (value: unknown) => void;
  const promise = new Promise<unknown>((resolve) => (release = resolve));
  backend.held.set(key, { promise, release });
  return { release: (value?: unknown) => release(value) };
}

const calls = (command: string) => backend.calls.filter((c) => c.command === command);

describe('a second trigger while the first is running', () => {
  beforeEach(() => {
    backend.held.clear();
    backend.calls.length = 0;
    debouncedSetHistory.cancel();
  });

  it('Generate starts one job, not two', async () => {
    const patch = { id: 'p1', name: 'Patch', visible: true, invert: false, prompt: '', isLoading: false, subMasks: [] };
    const image = { path: '/p/a.CR2', isReady: true, exif: {} } as unknown as SelectedImage;
    const adjustments = { ...INITIAL_ADJUSTMENTS, aiPatches: [patch as never] };
    useEditorStore.setState({ selectedImage: image, adjustments, history: [adjustments], historyIndex: 0 });
    const answer = hold(Invokes.InvokeGenerativeReplaseWithMaskDef);
    const { handleGenerativeReplace } = hook(useAiMasking);
    const first = handleGenerativeReplace('p1', 'a cat', false);
    const second = handleGenerativeReplace('p1', 'a cat', false);
    expect(calls(Invokes.InvokeGenerativeReplaseWithMaskDef)).toHaveLength(1);
    answer.release(JSON.stringify({ png: 'x' }));
    await first;
    await second;
    // Once the first is done the next press works.
    backend.held.delete(Invokes.InvokeGenerativeReplaseWithMaskDef);
    await handleGenerativeReplace('p1', 'a cat', false);
    expect(calls(Invokes.InvokeGenerativeReplaseWithMaskDef)).toHaveLength(2);
  });

  it('Paste files copies once', async () => {
    useProcessStore.setState({ copiedFilePaths: ['/p/a.CR2'] });
    useLibraryStore.setState({ currentFolderPath: '/dest' });
    const answer = hold(Invokes.CopyFiles);
    const { handlePasteFiles } = hook(() =>
      useFileOperations(
        async () => {},
        async () => {},
        () => {},
        () => {},
        [],
      ),
    );
    const first = handlePasteFiles('copy');
    const second = handlePasteFiles('copy');
    expect(calls(Invokes.CopyFiles)).toHaveLength(1);
    answer.release();
    await first;
    await second;
  });
});
