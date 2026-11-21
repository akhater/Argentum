// hooks/useFileOperations.ts asks argentum/fileOps.ts what to show after a delete or a rename.
// These run the hook itself, with the backend answering, to prove it acts on the answer.
import { beforeEach, describe, expect, it, vi } from 'vitest';

const backend = vi.hoisted(() => ({ answers: new Map<string, unknown>(), calls: [] as unknown[][] }));

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (command: string, args?: unknown) => {
    backend.calls.push([command, args]);
    return Promise.resolve(backend.answers.get(command));
  },
}));

// The hook uses no React hook but useCallback, so it can be called outside a component.
vi.mock('react', async (importOriginal) => ({
  ...(await importOriginal<typeof import('react')>()),
  useCallback: <T>(fn: T) => fn,
}));
vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

import { useFileOperations } from '../hooks/useFileOperations';
import { useEditorStore } from '../store/useEditorStore';
import { useLibraryStore } from '../store/useLibraryStore';
import { useSettingsStore } from '../store/useSettingsStore';
import { useUIStore } from '../store/useUIStore';
import { useModernFolders } from './modern/folders';
import { Invokes, type SelectedImage } from '../components/ui/AppProperties';

const shown = ['/p/a.CR2', '/p/b.CR2', '/p/b.CR2?vc=a1b2c3', '/p/c.CR2'];
const handleImageSelect = vi.fn();
const handleBackToLibrary = vi.fn();
const refreshImageList = vi.fn(async () => {});
const fileOperations = () =>
  useFileOperations(
    refreshImageList,
    async () => {},
    handleImageSelect,
    handleBackToLibrary,
    shown.map((path) => ({ path })),
  );

describe('the library after a file operation', () => {
  beforeEach(() => {
    backend.answers.clear();
    backend.calls.length = 0;
    handleImageSelect.mockReset();
    handleBackToLibrary.mockReset();
    refreshImageList.mockClear();
    useUIStore.setState({ activeView: 'library' });
    useEditorStore.setState({ selectedImage: null });
    useLibraryStore.setState({ libraryActivePath: null, multiSelectedPaths: [] });
  });

  it('selects the next photo after a delete, not a virtual copy deleted with it', async () => {
    useLibraryStore.setState({ libraryActivePath: '/p/b.CR2', multiSelectedPaths: ['/p/b.CR2'] });

    await fileOperations().executeDelete(['/p/b.CR2']);

    expect(useLibraryStore.getState().libraryActivePath).toBe('/p/c.CR2');
    expect(useLibraryStore.getState().multiSelectedPaths).toEqual(['/p/c.CR2']);
  });

  it('keeps the grid when the photo shown there is renamed', async () => {
    useEditorStore.setState({ selectedImage: { path: '/p/a.CR2' } as SelectedImage });
    useUIStore.setState({ renameTargetPaths: ['/p/a.CR2'] });
    backend.answers.set(Invokes.RenameFiles, ['/p/trip_1.CR2']);

    await fileOperations().handleSaveRename('trip_{sequence}');

    expect(handleImageSelect).toHaveBeenCalledWith('/p/trip_1.CR2', false);
    expect(handleBackToLibrary).not.toHaveBeenCalled();
  });

  it('moves only the folders inside a renamed folder', async () => {
    const handleSettingsChange = vi.fn(async () => {});
    useSettingsStore.setState({
      appSettings: { pinnedFolders: ['/photos/trip/day 1', '/photos/trip2'] } as never,
      handleSettingsChange,
    });
    useLibraryStore.setState({ rootPaths: ['/photos'], currentFolderPath: '/photos/trip2' });
    useUIStore.setState({ folderActionTarget: '/photos/trip' });

    await fileOperations().handleRenameFolder('rome');

    expect(useLibraryStore.getState().currentFolderPath).toBe('/photos/trip2');
    expect(handleSettingsChange).toHaveBeenCalledWith(
      expect.objectContaining({ pinnedFolders: ['/photos/rome/day 1', '/photos/trip2'] }),
    );
  });

  it('lists the photos again when a folder inside the one on show is renamed', async () => {
    useSettingsStore.setState({ appSettings: {} as never, handleSettingsChange: vi.fn(async () => {}) });
    useLibraryStore.setState({ rootPaths: ['/photos'], currentFolderPath: '/photos' });
    useUIStore.setState({ folderActionTarget: '/photos/trip' });

    await fileOperations().handleRenameFolder('rome');

    // The recursive view still listed /photos/trip's photos under their old paths.
    expect(refreshImageList).toHaveBeenCalled();
    expect(useLibraryStore.getState().currentFolderPath).toBe('/photos');
  });

  it('renames the folder in the Recent list too', async () => {
    useSettingsStore.setState({ appSettings: {} as never, handleSettingsChange: vi.fn(async () => {}) });
    useLibraryStore.setState({ rootPaths: ['/photos'], currentFolderPath: '/photos/trip' });
    useUIStore.setState({ folderActionTarget: '/photos/trip' });
    useModernFolders.setState({ recent: ['/photos/trip', '/photos/x'] });
    backend.answers.set('ag', ['/photos/rome', '/photos/x']);

    await fileOperations().handleRenameFolder('rome');

    expect(backend.calls).toContainEqual([
      'ag',
      { name: 'rename_recent_folder', args: { oldPath: '/photos/trip', newPath: '/photos/rome' } },
    ]);
    expect(useModernFolders.getState().recent).toEqual(['/photos/rome', '/photos/x']);
    expect(useLibraryStore.getState().currentFolderPath).toBe('/photos/rome');
  });
});
