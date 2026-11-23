// A photo whose edit file exists but cannot be read must not open as a blank edit that a save would
// write over the real one: the open fails like any image that cannot load (hooks/useImageLoader.ts),
// and a batch reload that fails leaves the open edit as it is (hooks/useEditorActions.ts).
import { beforeEach, describe, expect, it, vi } from 'vitest';

const backend = vi.hoisted(() => ({
  calls: [] as string[],
  metadata: undefined as unknown,
}));

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (command: string) => {
    backend.calls.push(command);
    if (command === 'load_metadata') {
      return backend.metadata instanceof Error ? Promise.reject(backend.metadata) : Promise.resolve(backend.metadata);
    }
    return Promise.resolve(undefined);
  },
}));
vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
const toast = vi.hoisted(() => ({ error: vi.fn(), info: vi.fn() }));
vi.mock('react-toastify', () => ({ toast }));

import { loadEditOnOpen } from '../hooks/useImageLoader';
import { reloadAdjustmentsForPaths } from '../hooks/useEditorActions';
import { useEditorStore } from '../store/useEditorStore';
import { useLibraryStore } from '../store/useLibraryStore';
import type { SelectedImage } from '../components/ui/AppProperties';
import { INITIAL_ADJUSTMENTS, type Adjustments } from '../utils/adjustments';

const real: Adjustments = { ...INITIAL_ADJUSTMENTS, exposure: 1 };
const unreadable = new Error('The edit of /p/a.CR2 could not be read. Try again in a moment.');

function opening(path: string) {
  useEditorStore.setState({
    selectedImage: { path, isReady: false } as SelectedImage,
    adjustments: real,
    history: [real],
    historyIndex: 0,
  });
  useLibraryStore.setState({ isViewLoading: true });
}

beforeEach(() => {
  backend.calls = [];
  backend.metadata = undefined;
  toast.error.mockClear();
});

describe('opening a photo', () => {
  it('whose edit cannot be read leaves no photo open and no blank edit to save', async () => {
    opening('/p/a.CR2');
    backend.metadata = unreadable;

    const loaded = await loadEditOnOpen('/p/a.CR2', () => true);

    expect(loaded).toBe(false);
    expect(useEditorStore.getState().selectedImage).toBeNull();
    expect(useLibraryStore.getState().isViewLoading).toBe(false);
    expect(toast.error).toHaveBeenCalledTimes(1);
    expect(backend.calls.filter((c) => c.includes('save'))).toEqual([]);
  });

  it('whose edit reads puts it in the editor', async () => {
    opening('/p/a.CR2');
    backend.metadata = { adjustments: { exposure: 2 } };

    expect(await loadEditOnOpen('/p/a.CR2', () => true)).toBe(true);

    expect(useEditorStore.getState().adjustments.exposure).toBe(2);
    expect(useEditorStore.getState().selectedImage?.path).toBe('/p/a.CR2');
  });

  it('after the user moved on is left alone when the load fails', async () => {
    opening('/p/b.CR2');
    backend.metadata = unreadable;

    await loadEditOnOpen('/p/a.CR2', () => false);

    expect(useEditorStore.getState().selectedImage?.path).toBe('/p/b.CR2');
    expect(toast.error).not.toHaveBeenCalled();
  });
});

describe('a batch reload that fails', () => {
  it('leaves the open edit as it is', async () => {
    useEditorStore.setState({
      selectedImage: { path: '/p/a.CR2', isReady: true } as SelectedImage,
      adjustments: real,
      history: [INITIAL_ADJUSTMENTS, real],
      historyIndex: 1,
    });
    backend.metadata = unreadable;

    await expect(reloadAdjustmentsForPaths(['/p/a.CR2'])).rejects.toBe(unreadable);

    const state = useEditorStore.getState();
    expect(state.adjustments).toBe(real);
    expect(state.history).toHaveLength(2);
  });
});
