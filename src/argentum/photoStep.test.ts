// Left and Right and the thumbnail bar's arrows: one step, which opens in the editor and selects in the grid.
import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({ invoke: () => Promise.resolve() }));

import { LibraryViewMode, type ImageFile, type SelectedImage } from '../components/ui/AppProperties';
import { useEditorStore } from '../store/useEditorStore';
import { useLibraryStore } from '../store/useLibraryStore';
import { useSettingsStore } from '../store/useSettingsStore';
import { useUIStore } from '../store/useUIStore';
import { arrowsAt, stepPhoto } from './photoStep';

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

describe('where the arrows stop', () => {
  const flat = (shown: ImageFile[], path: string) => arrowsAt(shown, path, 'library', LibraryViewMode.Flat);
  const both = { previous: true, next: true };
  const neither = { previous: false, next: false };

  it('stops the previous arrow on the first photo and the next arrow on the last', () => {
    expect(flat(list, '/p/a.CR2')).toEqual({ previous: false, next: true });
    expect(flat(list, '/p/b.CR2')).toEqual(both);
    expect(flat(list, '/p/c.CR2')).toEqual({ previous: true, next: false });
  });

  it('stops both with one photo, and with none', () => {
    expect(flat(images('/p/a.CR2'), '/p/a.CR2')).toEqual(neither);
    expect(flat([], '/p/a.CR2')).toEqual(neither);
  });

  it('goes by the one folder in a recursive grid, and the whole list in the editor', () => {
    const mixed = images('/p/a.CR2', '/q/x.CR2', '/p/b.CR2');
    const recursive = (path: string, view = 'library') => arrowsAt(mixed, path, view, LibraryViewMode.Recursive);

    expect(recursive('/p/a.CR2')).toEqual({ previous: false, next: true });
    expect(recursive('/p/b.CR2')).toEqual({ previous: true, next: false });
    expect(recursive('/q/x.CR2')).toEqual(neither);
    expect(recursive('/q/x.CR2', 'editor')).toEqual(both);
  });

  it('leaves both on for a photo that left the list, as there is no end to be at', () => {
    expect(flat(list, '/p/gone.CR2')).toEqual(both);
    expect(flat(images('/p/a.CR2'), '/p/gone.CR2')).toEqual(both);
    expect(arrowsAt(images('/q/x.CR2'), '/p/gone.CR2', 'library', LibraryViewMode.Recursive)).toEqual(neither);
  });
});
