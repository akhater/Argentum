/**
 * Previous and next photo, for Left and Right and the thumbnail bar's arrows alike. Ours.
 *
 * The editor opens the photo. The grid selects it, and a recursive view keeps to the folder of the
 * photo on show; the editor walks the whole list. Any other view has no photo to step. The keys wrap
 * at the ends of that list and the arrows stop there, by `arrowsAt`.
 */

import { useCallback, useEffect, useMemo, useRef } from 'react';
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

/** The photos a step from `path` goes through in `activeView`. */
function stepOrder(
  list: ReadonlyArray<ImageFile>,
  path: string,
  activeView: string,
  viewMode: LibraryViewMode | undefined,
) {
  if (activeView !== 'library' || viewMode !== LibraryViewMode.Recursive) return list;
  return list.filter((image) => parentFolder(image.path) === parentFolder(path));
}

/**
 * Which arrows can step from `path`. A photo that has left the list has no end to be at: both go, to
 * the nearest photo, while there is one.
 */
export function arrowsAt(
  list: ReadonlyArray<ImageFile>,
  path: string,
  activeView: string,
  viewMode: LibraryViewMode | undefined,
) {
  const order = stepOrder(list, path, activeView, viewMode);
  const index = order.findIndex((image) => image.path === path);
  if (index === -1) return { previous: order.length > 0, next: order.length > 0 };
  return { previous: index > 0, next: index < order.length - 1 };
}

/** `arrowsAt` for the view on show; neither with no photo on show. */
export function useArrowsAt(list: ReadonlyArray<ImageFile>, path: string | null | undefined) {
  const activeView = useUIStore((s) => s.activeView);
  const viewMode = useSettingsStore((s) => s.appSettings?.libraryViewMode);
  return useMemo(
    () => (path ? arrowsAt(list, path, activeView, viewMode) : { previous: false, next: false }),
    [list, path, activeView, viewMode],
  );
}

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

  const viewMode = useSettingsStore.getState().appSettings?.libraryViewMode;
  const target = stepFrom(pathsOf(stepOrder(list, path, activeView, viewMode)), lastOrder, path, step);
  if (!target) return;
  if (activeView === 'editor') handleImageSelect(target, true);
  else selectInGrid(target, handleImageSelect);
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
