/**
 * Previous and next photo, for Left and Right and the thumbnail bar's arrows alike. Ours.
 *
 * The editor opens the photo. The grid selects it, and a recursive view keeps to the folder of the
 * photo on show; the editor walks the whole list. Any other view has no photo to step.
 */

import { useCallback, useEffect, useRef } from 'react';
import { type ImageFile, LibraryViewMode } from '../components/ui/AppProperties';
import { activePhotoPath } from '../hooks/useLibraryActions';
import { useLibraryStore } from '../store/useLibraryStore';
import { useSettingsStore } from '../store/useSettingsStore';
import { useUIStore } from '../store/useUIStore';
import { lastOrderHolding, stepFrom } from './nearestPhoto';
import { physicalPathOf } from './paths';

type ImageSelect = (path: string, openInEditor?: boolean) => void;

const pathsOf = (images: ReadonlyArray<ImageFile>) => images.map((image) => image.path);

const parentFolder = (path: string) => {
  const physicalPath = physicalPathOf(path);
  const separator = physicalPath.includes('/') ? '/' : '\\';
  const lastSep = physicalPath.lastIndexOf(separator);
  return lastSep === -1 ? physicalPath : physicalPath.slice(0, lastSep);
};

/** Makes `path` the grid's active photo, alone in the selection. */
export function selectInGrid(path: string, handleImageSelect: ImageSelect) {
  useLibraryStore.getState().setLibrary({ libraryActivePath: path, multiSelectedPaths: [path] });
  handleImageSelect(path, false);
}

/** Moves the photo on show by `step` through `list`; `lastOrder` is where it stood if it has left `list`. */
export function stepPhoto(
  list: ReadonlyArray<ImageFile>,
  lastOrder: readonly string[],
  step: 1 | -1,
  handleImageSelect: ImageSelect,
) {
  const { activeView } = useUIStore.getState();
  const path = activePhotoPath();
  if (!path || (activeView !== 'editor' && activeView !== 'library')) return;

  if (activeView === 'editor') {
    const target = stepFrom(pathsOf(list), lastOrder, path, step);
    if (target) handleImageSelect(target, true);
    return;
  }
  const recursive = useSettingsStore.getState().appSettings?.libraryViewMode === LibraryViewMode.Recursive;
  const neighbours = recursive ? list.filter((image) => parentFolder(image.path) === parentFolder(path)) : list;
  const target = stepFrom(pathsOf(neighbours), lastOrder, path, step);
  if (target) selectInGrid(target, handleImageSelect);
}

/** `stepPhoto` for the list on show, keeping the record of where the photo on show stood. */
export function usePhotoStep(list: ReadonlyArray<ImageFile>, handleImageSelect: ImageSelect) {
  const listRef = useRef(list);
  // The last list the photo on show was in: once it leaves the list, the only record of where it stood.
  const lastOrderRef = useRef<readonly string[]>([]);
  useEffect(() => {
    lastOrderRef.current = lastOrderHolding(lastOrderRef.current, pathsOf(listRef.current), activePhotoPath());
    listRef.current = list;
  }, [list]);

  return useCallback(
    (step: 1 | -1) => stepPhoto(listRef.current, lastOrderRef.current, step, handleImageSelect),
    [handleImageSelect],
  );
}
