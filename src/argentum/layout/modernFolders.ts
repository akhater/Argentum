/**
 * Where the Modern layout opens, and the folder column's own lists. Ours.
 *
 * No start screen. Where today's library shows its splash until a folder is
 * added or a session continued, Modern asks the backend where to open
 * (mods/modern_start.rs) and fills the folder column at once: every disk
 * under This computer in place of the folders added one by one, the recent
 * folders, and the folder last open, selected, if it is still there. The
 * folders added in Classic stay saved for Classic; Modern never changes them.
 */

import { invoke } from '@tauri-apps/api/core';
import { create } from 'zustand';
import { ag } from '../ag';
import { useEditorLayout } from './editorLayout';
import { useLibraryStore } from '../../store/useLibraryStore';
import { useSettingsStore } from '../../store/useSettingsStore';
import { useEditorStore } from '../../store/useEditorStore';
import { useProcessStore } from '../../store/useProcessStore';
import { Invokes, type ImageFile } from '../../components/ui/AppProperties';

/** A disk as mods/drives.rs lists it. Hand-written until the contract is generated. */
export interface Drive {
  path: string;
  label: string;
  removable: boolean;
  /** Zero where the system does not report it. */
  freeBytes: number;
  totalBytes: number;
}

/** mods/modern_start.rs `ModernStart`. Hand-written until the contract is generated. */
interface ModernStart {
  drives: Drive[];
  recent: string[];
  startFolder: string | null;
  expanded: string[];
}

interface ModernFoldersState {
  drives: Drive[];
  /** Newest first, only folders that are there. */
  recent: string[];
  /** No disk could be listed, so today's start screen shows instead. */
  unavailable: boolean;
}

export const useModernFolders = create<ModernFoldersState>(() => ({ drives: [], recent: [], unavailable: false }));

/** How the library writes an album in `currentFolderPath`. */
const ALBUM_PREFIX = 'Album: ';

export const isModern = () => useEditorLayout.getState().layout === 'modern';

/**
 * Whether unfolding a folder counts its photos. In Modern the tree's roots are
 * whole disks, and counting reads every folder under them, so no.
 */
export const rootImageCounts = (wanted: boolean) => wanted && !isModern();

/**
 * Fill the folder column, then open the folder last open through
 * `openFolder`. If no disk can be listed nothing changes, and the library
 * shows today's start screen.
 */
async function startModern(openFolder: (path: string) => Promise<void> | void): Promise<void> {
  const { appSettings } = useSettingsStore.getState();
  const { setLibrary } = useLibraryStore.getState();
  const saved = appSettings?.lastFolderState;

  const start = await listDisks(saved?.currentFolderPath ?? null, saved?.expandedFolders ?? []);
  const roots = start?.drives.map((d) => d.path) ?? [];
  if (!start || roots.length === 0) return;
  watchLibrary();

  setLibrary({ rootPaths: roots, expandedFolders: new Set(start.expanded), isTreeLoading: true });
  try {
    const trees = await invoke<unknown[]>(Invokes.GetPinnedFolderTrees, {
      paths: roots,
      expandedFolders: start.expanded,
      showImageCounts: false,
    });
    setLibrary({ folderTrees: trees });
  } catch (err) {
    console.error('Could not read the disks for the folder column:', err);
  } finally {
    setLibrary({ isTreeLoading: false });
  }

  if (start.startFolder) await openFolder(start.startFolder);
}

/**
 * Start the library again for the layout just chosen: its folders go, and
 * its own start brings them back that layout's way (Modern fills the column
 * at once, Classic shows its start screen).
 */
export function restartLibrary() {
  useModernFolders.setState({ unavailable: false });
  useLibraryStore.getState().setLibrary({ rootPaths: [], folderTrees: [] });
}

/** The disks and recent folders, as the backend sees them now. None if they could not be listed. */
async function listDisks(lastFolder: string | null, expanded: string[]): Promise<ModernStart | null> {
  try {
    const start = await ag<ModernStart>('modern_start', { lastFolder, expanded });
    useModernFolders.setState({ drives: start.drives, recent: start.recent, unavailable: start.drives.length === 0 });
    return start;
  } catch (err) {
    console.error('Could not list the disks:', err);
    useModernFolders.setState({ unavailable: true });
    return null;
  }
}

/**
 * Read the column again: the disks (one plugged in since shows up), without
 * photo counts, and the favourites as their setting says. Modern's stand-in
 * for the library's own refresh, which would count every folder on a disk.
 */
export async function refreshModernTrees(): Promise<void> {
  const { appSettings } = useSettingsStore.getState();
  const { expandedFolders, setLibrary } = useLibraryStore.getState();
  const expanded = Array.from(expandedFolders);
  const pinned = appSettings?.pinnedFolders ?? [];
  const start = await listDisks(null, []);
  if (!start || start.drives.length === 0) return;
  const roots = start.drives.map((d) => d.path);
  try {
    const [folderTrees, pinnedFolderTrees] = await Promise.all([
      invoke<unknown[]>(Invokes.GetPinnedFolderTrees, {
        paths: roots,
        expandedFolders: expanded,
        showImageCounts: false,
      }),
      pinned.length > 0
        ? invoke<unknown[]>(Invokes.GetPinnedFolderTrees, {
            paths: pinned,
            expandedFolders: expanded,
            showImageCounts: appSettings?.enableFolderImageCounts ?? false,
          })
        : Promise.resolve([]),
    ]);
    setLibrary({ rootPaths: roots, folderTrees, pinnedFolderTrees });
  } catch (err) {
    console.error('Failed to refresh the folder column:', err);
  }
}

/** What App knows about the first folder, read ahead while the app starts (useAppInitialization). */
interface Preloaded {
  currentPath?: string;
  images?: Promise<ImageFile[]>;
}

type SelectFolder = (
  path: string,
  isNewRoot?: boolean,
  preloadedImages?: ImageFile[],
  expandParents?: boolean,
  preserveEditor?: boolean,
) => Promise<void>;

/**
 * Modern's stand-in for Continue Session: fill the column and open the folder
 * last open, with the photos read ahead if they are of that folder. A photo
 * opened with Argentum stays open; the folder fills in behind it.
 */
export function startModernLibrary(preloaded: { current: Preloaded | null }, selectFolder: SelectFolder): void {
  startModern(async (path) => {
    const ahead = preloaded.current;
    let images: ImageFile[] | undefined;
    if (ahead?.currentPath === path && ahead.images) {
      images = await ahead.images.catch(() => undefined);
      ahead.images = undefined;
    }
    const { initialFileToOpen, externalEditSession } = useProcessStore.getState();
    const keepEditor = !!(useEditorStore.getState().selectedImage || initialFileToOpen || externalEditSession);
    await selectFolder(path, false, images, false, keepEditor);
  }).catch((err) => console.error('Failed to open the Modern folder column:', err));
}

let watching = false;

/**
 * While Modern is chosen: every folder opened goes to the top of Recent, and
 * the tree's roots stay the disks. The library still loads the folders added
 * in Classic when its photo-count setting changes; if that or anything else
 * puts them in the tree, the disks are read back.
 */
function watchLibrary() {
  if (watching) return;
  watching = true;
  useLibraryStore.subscribe((state, previous) => {
    if (!isModern()) return;
    const folder = state.currentFolderPath;
    if (folder && folder !== previous.currentFolderPath && !folder.startsWith(ALBUM_PREFIX)) {
      ag<string[]>('add_recent_folder', { path: folder })
        .then((recent) => useModernFolders.setState({ recent }))
        .catch((err) => console.warn('Recent folders not saved:', err));
    }
    if (state.folderTrees !== previous.folderTrees) {
      const disks = useModernFolders.getState().drives.map((d) => d.path);
      const roots = state.folderTrees.map((tree: { path?: string }) => tree.path);
      if (disks.length > 0 && roots.some((root) => !root || !disks.includes(root))) {
        refreshModernTrees();
      }
    }
  });
}

/** A disk as the column names it: its letter or mount point, then its own name. */
export function driveName(drive: Drive): string {
  const where = drive.path.replace(/[\\/]+$/, '') || drive.path;
  return drive.label ? `${where}  ${drive.label}` : where;
}

/** The last part of a folder's path, for the Recent list. */
export function folderName(path: string): string {
  const parts = path.split(/[\\/]+/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}
