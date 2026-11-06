// Opening folders quickly: the folder opened last is the one shown, even when an earlier
// one is still being listed (a slow or sleeping drive) and answers after it.
// handleSelectSubfolder in hooks/useAppNavigation.ts; the listing runs off the UI thread.
import { beforeEach, describe, expect, it, vi } from 'vitest';

const backend = vi.hoisted(() => ({
  /** Listings held back, by folder: answer with photos, or fail. */
  held: new Map<string, { answer: (files: unknown[]) => void; fail: (error: string) => void }>(),
  slow: new Set<string>(),
  albums: new Map<string, { answer: (files: unknown[]) => void }>(),
}));

const LISTINGS = ['list_images_in_dir', 'list_images_recursive'];

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (command: string, args?: { path?: string; paths?: string[] }) => {
    const path = args?.path ?? '';
    if (LISTINGS.includes(command)) {
      if (backend.slow.has(path)) {
        return new Promise((answer, fail) => backend.held.set(`${command} ${path}`, { answer, fail }));
      }
      return Promise.resolve([photo(`${path}/fast.CR3`)]);
    }
    if (command === 'get_album_images') {
      return new Promise((answer) => backend.albums.set(String(args?.paths), { answer }));
    }
    if (command === 'read_exif_for_paths') return Promise.resolve({});
    return Promise.resolve(undefined);
  },
}));

const toastError = vi.hoisted(() => vi.fn());
vi.mock('react-toastify', () => ({ toast: { error: toastError } }));

// The hook uses no React hook but useCallback, so it can be called outside a component.
vi.mock('react', async (importOriginal) => ({
  ...(await importOriginal<typeof import('react')>()),
  useCallback: <T>(fn: T) => fn,
}));

import { useAppNavigation } from '../hooks/useAppNavigation';
import { useLibraryStore } from '../store/useLibraryStore';
import { useSettingsStore } from '../store/useSettingsStore';
import { LibraryViewMode } from '../components/ui/AppProperties';

function photo(path: string) {
  return { path, modified: 0, rating: 0, is_virtual_copy: false, is_raw: true, tags: null, exif: null };
}

const ref = <T>(current: T) => ({ current });
const navigation = (preloadedDataRef: { current: unknown } = ref(null)) =>
  useAppNavigation({
    clearThumbnailQueue: () => {},
    refs: {
      transformWrapperRef: ref(null),
      preloadedDataRef,
      cachedEditStateRef: ref(null),
      selectedImagePathRef: ref(null),
      isBackendReadyRef: ref(false),
      latestRenderedJobIdRef: ref(0),
      previewJobIdRef: ref(0),
      currentResRef: ref(0),
      prevAdjustmentsRef: ref(null),
    },
  });

const shown = () => useLibraryStore.getState().imageList.map((f) => f.path);
const library = () => useLibraryStore.getState();

describe.each([
  [LibraryViewMode.Flat, 'list_images_in_dir'],
  [LibraryViewMode.Recursive, 'list_images_recursive'],
])('opening folders quickly, %s', (libraryViewMode, command) => {
  const held = (folder: string) => backend.held.get(`${command} ${folder}`)!;

  beforeEach(() => {
    backend.held.clear();
    backend.slow.clear();
    toastError.mockClear();
    useSettingsStore.setState({ appSettings: { libraryViewMode } as never });
    useLibraryStore.setState({ currentFolderPath: null, imageList: [], isViewLoading: false });
  });

  it('keeps the folder opened last when an earlier one answers late', async () => {
    backend.slow.add('A');
    const { handleSelectSubfolder } = navigation();

    const first = handleSelectSubfolder('A', false, undefined, false);
    await vi.waitFor(() => expect(held('A')).toBeDefined());
    await handleSelectSubfolder('B', false, undefined, false);
    held('A').answer([photo('A/late.CR3')]);
    await first;

    expect(library().currentFolderPath).toBe('B');
    expect(shown()).toEqual(['B/fast.CR3']);
    expect(library().isViewLoading).toBe(false);
  });

  it('still loads while the folder opened last is listed, when an earlier one answers', async () => {
    backend.slow.add('A');
    backend.slow.add('B');
    const { handleSelectSubfolder } = navigation();

    const first = handleSelectSubfolder('A', false, undefined, false);
    await vi.waitFor(() => expect(held('A')).toBeDefined());
    const second = handleSelectSubfolder('B', false, undefined, false);
    await vi.waitFor(() => expect(held('B')).toBeDefined());
    held('A').answer([photo('A/late.CR3')]);
    await first;

    expect(shown()).toEqual([]);
    expect(library().isViewLoading).toBe(true);

    held('B').answer([photo('B/b.CR3')]);
    await second;
    expect(shown()).toEqual(['B/b.CR3']);
    expect(library().isViewLoading).toBe(false);
  });

  it('says nothing about an earlier folder that fails after another was opened', async () => {
    backend.slow.add('A');
    const { handleSelectSubfolder } = navigation();

    const first = handleSelectSubfolder('A', false, undefined, false);
    await vi.waitFor(() => expect(held('A')).toBeDefined());
    await handleSelectSubfolder('B', false, undefined, false);
    held('A').fail('The network path was not found.');
    await first;

    expect(toastError).not.toHaveBeenCalled();
    expect(shown()).toEqual(['B/fast.CR3']);
  });

  it('still tells about the folder opened last when it fails', async () => {
    backend.slow.add('A');
    const { handleSelectSubfolder } = navigation();

    const only = handleSelectSubfolder('A', false, undefined, false);
    await vi.waitFor(() => expect(held('A')).toBeDefined());
    held('A').fail('The network path was not found.');
    await only;

    expect(toastError).toHaveBeenCalledOnce();
    expect(library().isViewLoading).toBe(false);
  });

  it('keeps an album opened after a folder that answers late', async () => {
    backend.slow.add('A');
    const { handleSelectSubfolder, handleSelectAlbum } = navigation();

    const folder = handleSelectSubfolder('A', false, undefined, false);
    await vi.waitFor(() => expect(held('A')).toBeDefined());
    const album = handleSelectAlbum('album-1', 'Trip', ['X/x.CR3']);
    await vi.waitFor(() => expect(backend.albums.get('X/x.CR3')).toBeDefined());
    backend.albums.get('X/x.CR3')!.answer([photo('X/x.CR3')]);
    await album;
    held('A').answer([photo('A/late.CR3')]);
    await folder;

    expect(library().currentFolderPath).toBe('Album: Trip');
    expect(shown()).toEqual(['X/x.CR3']);
  });
});

describe('a folder opened while the one last open is listed at start', () => {
  beforeEach(() => {
    backend.held.clear();
    backend.slow.clear();
    useSettingsStore.setState({
      appSettings: {
        libraryViewMode: LibraryViewMode.Flat,
        rootFolders: ['R'],
        lastFolderState: { currentFolderPath: 'R/Last' },
      } as never,
    });
    useLibraryStore.setState({ currentFolderPath: null, imageList: [], isViewLoading: false });
  });

  /** The first folder's listing, started with the app: the session waits on it. */
  const readAhead = () => {
    const listing = { waiting: false, answer: (_: unknown[]) => {} };
    const images = {
      then: (answer: (files: unknown[]) => void) => {
        listing.waiting = true;
        listing.answer = answer;
      },
    };
    return { listing, preloaded: ref({ rootPaths: ['R'], trees: Promise.resolve([]), currentPath: 'R/Last', images }) };
  };
  const settle = () => new Promise((resolve) => setTimeout(resolve, 20));

  it('stays open when the one last open answers after it', async () => {
    const { listing, preloaded } = readAhead();
    const { handleContinueSession, handleSelectSubfolder } = navigation(preloaded);

    handleContinueSession();
    await vi.waitFor(() => expect(listing.waiting).toBe(true));
    await handleSelectSubfolder('B', false, undefined, false);
    listing.answer([photo('R/Last/late.CR3')]);
    await settle();

    expect(library().currentFolderPath).toBe('B');
    expect(shown()).toEqual(['B/fast.CR3']);
  });

  it('is not needed for the one last open to show, with its photos read ahead', async () => {
    const { listing, preloaded } = readAhead();
    const { handleContinueSession } = navigation(preloaded);

    handleContinueSession();
    await vi.waitFor(() => expect(listing.waiting).toBe(true));
    listing.answer([photo('R/Last/ahead.CR3')]);
    await settle();

    expect(library().currentFolderPath).toBe('R/Last');
    expect(shown()).toEqual(['R/Last/ahead.CR3']);
  });
});
