// A photo handed in from outside takes the library to its own folder, whichever got there first.
import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({ invoke: () => Promise.resolve([]) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: () => Promise.resolve(() => {}) }));
vi.mock('../ag', () => ({
  ag: () => Promise.resolve({ drives: [{ path: 'C:\\' }], recent: [], startFolder: 'C:\\Inbox\\Last', expanded: [] }),
}));

import { useEditorStore } from '../../store/useEditorStore';
import { useLibraryStore } from '../../store/useLibraryStore';
import { useProcessStore } from '../../store/useProcessStore';
import { useSettingsStore } from '../../store/useSettingsStore';
import { folderOfPhoto, openFromOutside, startModernLibrary } from './folders';
import { newFolderRequest } from '../folderRequest';
import type { ImageFile } from '../../components/ui/AppProperties';

const openPhoto = vi.fn();
const showFolder = vi.fn();
const photo = 'C:\\Inbox\\2026-10-27\\IMG_3006.CR3';

describe('the folder of a photo', () => {
  it('is the folder it sits in, on every kind of path', () => {
    expect(folderOfPhoto(photo)).toBe('C:\\Inbox\\2026-10-27');
    expect(folderOfPhoto('/Volumes/Trips/IMG_3006.CR3')).toBe('/Volumes/Trips');
    expect(folderOfPhoto('\\\\nas\\photos\\IMG_3006.CR3')).toBe('\\\\nas\\photos');
  });

  it('is a drive or a disk itself for a photo at its root', () => {
    expect(folderOfPhoto('E:\\IMG_3006.CR3')).toBe('E:\\');
    expect(folderOfPhoto('/IMG_3006.CR3')).toBe('/');
  });
});

describe('a photo handed in from outside', () => {
  beforeEach(() => {
    openPhoto.mockReset();
    showFolder.mockReset();
    useLibraryStore.setState({ currentFolderPath: null });
    useEditorStore.setState({ selectedImage: null });
    useProcessStore.setState({ initialFileToOpen: null, externalEditSession: null } as never);
    useSettingsStore.setState({ appSettings: { lastFolderState: { currentFolderPath: 'C:\\Inbox\\Last' } } as never });
  });

  it('takes a running window to its folder and opens', () => {
    useLibraryStore.setState({ currentFolderPath: 'C:\\Inbox\\2026-11-01' });
    openFromOutside(photo, openPhoto, showFolder);
    expect(openPhoto).toHaveBeenCalledWith(photo);
    expect(showFolder).toHaveBeenCalledWith('C:\\Inbox\\2026-10-27');
  });

  it('leaves the library alone when its folder is already on show', () => {
    useLibraryStore.setState({ currentFolderPath: 'C:\\Inbox\\2026-10-27' });
    openFromOutside(photo, openPhoto, showFolder);
    expect(openPhoto).toHaveBeenCalledWith(photo);
    expect(showFolder).not.toHaveBeenCalled();
  });

  it('keeps its folder when the last one is restored after it', async () => {
    useLibraryStore.setState({ currentFolderPath: 'C:\\Inbox\\2026-10-27' });
    const selectFolder = vi.fn().mockResolvedValue(undefined);
    startModernLibrary({ current: null }, selectFolder);
    await vi.waitFor(() => expect(selectFolder).toHaveBeenCalled());
    expect(selectFolder).toHaveBeenCalledWith('C:\\Inbox\\2026-10-27', false, undefined, true, true);
  });

  it('with no photo, a start opens the folder last open', async () => {
    const selectFolder = vi.fn().mockResolvedValue(undefined);
    startModernLibrary({ current: null }, selectFolder);
    await vi.waitFor(() => expect(selectFolder).toHaveBeenCalled());
    expect(selectFolder).toHaveBeenCalledWith('C:\\Inbox\\Last', false, undefined, false, false);
  });

  /** The folder last open, listed while the app starts: the column waits on it. */
  const readAhead = () => {
    let answer: (files: ImageFile[]) => void = () => {};
    const images = new Promise<ImageFile[]>((resolve) => (answer = resolve));
    const waiting = vi.spyOn(images, 'catch');
    return { answer: (files: ImageFile[]) => answer(files), waiting, images };
  };

  it('opens the folder last open with its photos read ahead', async () => {
    const ahead = readAhead();
    const selectFolder = vi.fn().mockResolvedValue(undefined);
    startModernLibrary({ current: { currentPath: 'C:\\Inbox\\Last', images: ahead.images } }, selectFolder);
    await vi.waitFor(() => expect(ahead.waiting).toHaveBeenCalled());
    const files = [{ path: 'C:\\Inbox\\Last\\a.CR3' }] as ImageFile[];
    ahead.answer(files);
    await vi.waitFor(() => expect(selectFolder).toHaveBeenCalled());
    expect(selectFolder).toHaveBeenCalledWith('C:\\Inbox\\Last', false, files, false, false);
  });

  it('keeps a folder opened while the one last open is still being listed', async () => {
    const ahead = readAhead();
    const selectFolder = vi.fn().mockResolvedValue(undefined);
    startModernLibrary({ current: { currentPath: 'C:\\Inbox\\Last', images: ahead.images } }, selectFolder);
    await vi.waitFor(() => expect(ahead.waiting).toHaveBeenCalled());
    newFolderRequest(); // what opening another folder does
    ahead.answer([]);
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(selectFolder).not.toHaveBeenCalled();
  });
});
