// A delete the Recycle Bin cannot take asks before it deletes for good (permanentDelete.ts).
import { beforeEach, describe, expect, it, vi } from 'vitest';

const backend = vi.hoisted(() => ({ answers: new Map<string, unknown>(), calls: [] as unknown[][] }));

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (command: string, args?: unknown) => {
    backend.calls.push([command, args]);
    return Promise.resolve(backend.answers.get(command));
  },
}));
vi.mock('react', async (importOriginal) => ({
  ...(await importOriginal<typeof import('react')>()),
  useCallback: <T>(fn: T) => fn,
}));
vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string, options?: { count?: number }) => `${key}:${options?.count ?? ''}` }),
}));

import { useFileOperations } from '../hooks/useFileOperations';
import { useEditorStore } from '../store/useEditorStore';
import { useLibraryStore } from '../store/useLibraryStore';
import { useUIStore } from '../store/useUIStore';

const shown = ['/p/a.CR2', '/p/b.CR2', '/p/c.CR2'];
const refreshImageList = vi.fn(async () => {});
const operations = () =>
  useFileOperations(
    refreshImageList,
    async () => {},
    vi.fn(),
    vi.fn(),
    shown.map((path) => ({ path })),
  );
const permanentCalls = () => backend.calls.filter(([command]) => command === 'delete_permanently');
const refused = { status: 'cannot_recycle', files: ['/p/b.CR2', '/p/b.CR2.agdata'], recycled: 0 };

describe('a delete the Recycle Bin cannot take', () => {
  beforeEach(() => {
    backend.answers.clear();
    backend.calls.length = 0;
    refreshImageList.mockClear();
    useUIStore.setState({ activeView: 'library', confirmModalState: { isOpen: false } });
    useEditorStore.setState({ selectedImage: null });
    useLibraryStore.setState({ libraryActivePath: '/p/b.CR2', multiSelectedPaths: ['/p/b.CR2'] });
    backend.answers.set('delete_files_from_disk', refused);
  });

  it('asks, with Cancel as the default, and deletes nothing more until told', async () => {
    await operations().executeDelete(['/p/b.CR2']);

    const dialog = useUIStore.getState().confirmModalState;
    expect(dialog.isOpen).toBe(true);
    expect(dialog.focusCancel).toBe(true);
    expect(dialog.confirmVariant).toBe('destructive');
    expect(dialog.message).toBe('library.permanentDelete.message:2');
    expect(permanentCalls()).toEqual([]);
  });

  it('leaves the selection alone while the question is open or when Cancel answers it', async () => {
    await operations().executeDelete(['/p/b.CR2']);

    expect(useLibraryStore.getState().multiSelectedPaths).toEqual(['/p/b.CR2']);
    expect(useLibraryStore.getState().libraryActivePath).toBe('/p/b.CR2');
    expect(permanentCalls()).toEqual([]);
  });

  it('deletes for good once on a yes, even if the button is hit twice, then moves on', async () => {
    await operations().executeDelete(['/p/b.CR2']);
    const { onConfirm } = useUIStore.getState().confirmModalState;

    onConfirm?.();
    onConfirm?.();
    await vi.waitFor(() => expect(useLibraryStore.getState().libraryActivePath).toBe('/p/c.CR2'));

    expect(permanentCalls()).toEqual([['delete_permanently', { files: refused.files, paths: ['/p/b.CR2'] }]]);
  });

  it('refreshes the list at once when the bin took some of the files', async () => {
    backend.answers.set('delete_files_from_disk', { ...refused, recycled: 1 });
    await operations().executeDelete(['/p/b.CR2']);
    expect(refreshImageList).toHaveBeenCalledTimes(1);
  });

  it('asks the same for Delete with associated files', async () => {
    backend.answers.set('delete_files_with_associated', refused);
    await operations().executeDelete(['/p/b.CR2'], { includeAssociated: true });
    expect(useUIStore.getState().confirmModalState.isOpen).toBe(true);
    expect(permanentCalls()).toEqual([]);
  });

  it('asks nothing when the bin took everything', async () => {
    backend.answers.set('delete_files_from_disk', { status: 'done' });
    await operations().executeDelete(['/p/b.CR2']);
    expect(useUIStore.getState().confirmModalState.isOpen).toBe(false);
    expect(useLibraryStore.getState().libraryActivePath).toBe('/p/c.CR2');
  });
});
