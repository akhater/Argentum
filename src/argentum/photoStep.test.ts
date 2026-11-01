// Left and Right and the thumbnail bar's arrows: one step, which opens in the editor and selects in the grid.
import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({ invoke: () => Promise.resolve() }));

import { LibraryViewMode, type ImageFile, type SelectedImage } from '../components/ui/AppProperties';
import { useEditorStore } from '../store/useEditorStore';
import { useLibraryStore } from '../store/useLibraryStore';
import { useSettingsStore } from '../store/useSettingsStore';
import { useUIStore } from '../store/useUIStore';
import { stepPhoto } from './photoStep';

const images = (...paths: string[]) => paths.map((path) => ({ path })) as ImageFile[];
const list = images('/p/a.CR2', '/p/b.CR2', '/p/c.CR2');
const handleImageSelect = vi.fn();

const inGrid = (path: string | null) => {
  useUIStore.setState({ activeView: 'library' });
  useLibraryStore.setState({ libraryActivePath: path, multiSelectedPaths: path ? [path] : [] });
};
const inEditor = (path: string) => {
  useUIStore.setState({ activeView: 'editor' });
  useEditorStore.setState({ selectedImage: { path } as SelectedImage });
};
const viewMode = (libraryViewMode: LibraryViewMode) =>
  useSettingsStore.setState({ appSettings: { libraryViewMode } as never });

describe('stepping the photo on show', () => {
  beforeEach(() => {
    handleImageSelect.mockReset();
    useEditorStore.setState({ selectedImage: null });
    viewMode(LibraryViewMode.Flat);
  });

  it('opens the next and the previous photo in the editor', () => {
    inEditor('/p/b.CR2');
    stepPhoto(list, [], 1, handleImageSelect);
    expect(handleImageSelect).toHaveBeenLastCalledWith('/p/c.CR2', true);
    stepPhoto(list, [], -1, handleImageSelect);
    expect(handleImageSelect).toHaveBeenLastCalledWith('/p/a.CR2', true);
  });

  it('selects the next and the previous photo alone in the grid, without opening it', () => {
    inGrid('/p/b.CR2');
    useLibraryStore.setState({ multiSelectedPaths: ['/p/a.CR2', '/p/b.CR2'] });
    stepPhoto(list, [], 1, handleImageSelect);
    expect(useLibraryStore.getState().libraryActivePath).toBe('/p/c.CR2');
    expect(useLibraryStore.getState().multiSelectedPaths).toEqual(['/p/c.CR2']);
    expect(handleImageSelect).toHaveBeenLastCalledWith('/p/c.CR2', false);

    inGrid('/p/b.CR2');
    stepPhoto(list, [], -1, handleImageSelect);
    expect(useLibraryStore.getState().libraryActivePath).toBe('/p/a.CR2');
  });

  it('wraps at the ends of the list', () => {
    inEditor('/p/c.CR2');
    stepPhoto(list, [], 1, handleImageSelect);
    expect(handleImageSelect).toHaveBeenLastCalledWith('/p/a.CR2', true);

    inGrid('/p/a.CR2');
    stepPhoto(list, [], -1, handleImageSelect);
    expect(useLibraryStore.getState().libraryActivePath).toBe('/p/c.CR2');
  });

  it('steps from where a filtered-out photo stood', () => {
    const shown = images('/p/a.CR2', '/p/c.CR2');
    const stood = ['/p/a.CR2', '/p/b.CR2', '/p/c.CR2'];

    inGrid('/p/b.CR2');
    stepPhoto(shown, stood, -1, handleImageSelect);
    expect(useLibraryStore.getState().libraryActivePath).toBe('/p/a.CR2');

    inEditor('/p/b.CR2');
    stepPhoto(shown, stood, 1, handleImageSelect);
    expect(handleImageSelect).toHaveBeenLastCalledWith('/p/c.CR2', true);
  });

  it('stays in one folder in a recursive grid, and crosses folders in the editor', () => {
    const mixed = images('/p/a.CR2', '/q/x.CR2', '/p/b.CR2');
    viewMode(LibraryViewMode.Recursive);

    inGrid('/p/a.CR2');
    stepPhoto(mixed, [], 1, handleImageSelect);
    expect(useLibraryStore.getState().libraryActivePath).toBe('/p/b.CR2');

    inEditor('/p/a.CR2');
    stepPhoto(mixed, [], 1, handleImageSelect);
    expect(handleImageSelect).toHaveBeenLastCalledWith('/q/x.CR2', true);
  });

  it('does nothing with no photo on show, or an empty list', () => {
    inGrid(null);
    stepPhoto(list, [], 1, handleImageSelect);

    inGrid('/p/a.CR2');
    stepPhoto([], [], 1, handleImageSelect);

    expect(handleImageSelect).not.toHaveBeenCalled();
    expect(useLibraryStore.getState().libraryActivePath).toBe('/p/a.CR2');
  });
});
