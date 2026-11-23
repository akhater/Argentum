// A change made less than 50 ms before leaving the photo: the idle timer has queued nothing yet.
import { beforeEach, describe, expect, it, vi } from 'vitest';

const calls = vi.hoisted(() => [] as Array<{ command: string; args?: { path?: string; adjustments?: unknown } }>);

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (command: string, args?: { path?: string; adjustments?: unknown }) => {
    calls.push({ command, args });
    if (command === 'is_image_cached') return Promise.resolve(true);
    return new Promise(() => {});
  },
}));

vi.mock('react', async (importOriginal) => ({
  ...(await importOriginal<typeof import('react')>()),
  useCallback: <T>(fn: T) => fn,
}));

import { useAppNavigation } from '../hooks/useAppNavigation';
import { useEditorStore } from '../store/useEditorStore';
import { useLibraryStore } from '../store/useLibraryStore';
import { globalImageCache } from '../utils/ImageLRUCache';
import { INITIAL_ADJUSTMENTS } from '../utils/adjustments';
import { flushPendingSave, type QueuedEdit } from './pendingSave';

const saves = () => calls.filter((c) => c.command === 'save_metadata_and_update_thumbnail');

const opened = (path: string) =>
  globalImageCache.set(path, {
    adjustments: INITIAL_ADJUSTMENTS,
    histogram: null,
    waveform: null,
    finalPreviewUrl: null,
    uncroppedPreviewUrl: null,
    selectedImage: { path, isReady: true },
    originalSize: { width: 6000, height: 4000 },
    previewSize: { width: 1500, height: 1000 },
  });

const ref = <T>(current: T) => ({ current });
const navigation = (prev: { current: QueuedEdit | null }) =>
  useAppNavigation({
    clearThumbnailQueue: () => {},
    refs: {
      transformWrapperRef: ref(null),
      preloadedDataRef: ref(null),
      cachedEditStateRef: ref(null),
      selectedImagePathRef: ref<string | null>(null),
      isBackendReadyRef: ref(false),
      latestRenderedJobIdRef: ref(0),
      previewJobIdRef: ref(0),
      currentResRef: ref(0),
      prevAdjustmentsRef: prev,
    },
  });

describe('leaving the open photo with a change not yet queued', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    calls.length = 0;
    opened('b.cr3');
    useEditorStore.setState({
      selectedImage: { path: 'a.cr3', isReady: true } as never,
      adjustments: INITIAL_ADJUSTMENTS,
      previewOverride: null,
    });
    useLibraryStore.setState({ libraryActivePath: null, multiSelectedPaths: [] });
  });

  it('saves the change under the photo it was made on when another photo is chosen', async () => {
    const prev = ref<QueuedEdit | null>({ path: 'a.cr3', adjustments: INITIAL_ADJUSTMENTS });
    const edited = { ...INITIAL_ADJUSTMENTS, rotation: 3 };
    useEditorStore.setState({ adjustments: edited });
    vi.advanceTimersByTime(10);

    await navigation(prev).handleImageSelect('b.cr3', false);
    await vi.runAllTimersAsync();

    expect(saves()).toHaveLength(1);
    expect(saves()[0].args?.path).toBe('a.cr3');
    expect(saves()[0].args?.adjustments).toBe(edited);
  });

  it('writes nothing extra when nothing changed', async () => {
    const prev = ref<QueuedEdit | null>({ path: 'a.cr3', adjustments: INITIAL_ADJUSTMENTS });
    await navigation(prev).handleImageSelect('b.cr3', false);
    await vi.runAllTimersAsync();
    expect(saves()).toHaveLength(0);
  });

  it('does not queue the same change twice', async () => {
    const prev = ref<QueuedEdit | null>({ path: 'a.cr3', adjustments: INITIAL_ADJUSTMENTS });
    useEditorStore.setState({ adjustments: { ...INITIAL_ADJUSTMENTS, rotation: 3 } });
    flushPendingSave(prev);
    flushPendingSave(prev);
    await vi.runAllTimersAsync();
    expect(saves()).toHaveLength(1);
  });

  it('does not save an edit that was queued for another photo', async () => {
    const prev = ref<QueuedEdit | null>({ path: 'z.cr3', adjustments: INITIAL_ADJUSTMENTS });
    useEditorStore.setState({ adjustments: { ...INITIAL_ADJUSTMENTS, rotation: 3 } });
    flushPendingSave(prev);
    await vi.runAllTimersAsync();
    expect(saves()).toHaveLength(0);
  });
});
