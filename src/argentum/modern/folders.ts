/**
 * The Modern window's folder column: where it opens, the disks, the recent
 * folders. Ours.
 *
 * There is no start screen. Where today's library shows its splash until a
 * folder is added or a session continued, the Modern window asks the backend
 * where to open (mods/modern_start.rs) and fills the column at once: every
 * disk under This computer in place of folders added one by one, the recent
 * folders, and the folder last open, selected, if it is still there. The
 * folders added one by one before the Modern window stay in the settings; it
 * never changes them.
 *
 * A disk left folded is not read: its row is drawn from the list of disks, and
 * opening it is what reads it (mods/folder_walk.rs has the rule for the rows
 * below). Each disk read is its own call, so a slow one holds up only its row.
 */

import { invoke } from '@tauri-apps/api/core';
import { create } from 'zustand';
import { ag } from '../ag';
import { forgetFolderCounts } from './folderCounts';
import { rootContaining } from '../folderRoot';
import { parentFolder } from '../photoStep';
import { useLibraryStore } from '../../store/useLibraryStore';
import { useSettingsStore } from '../../store/useSettingsStore';
import { useEditorStore } from '../../store/useEditorStore';
import { useProcessStore } from '../../store/useProcessStore';
import { Invokes, type ImageFile } from '../../components/ui/AppProperties';
import type { Drive, ModernStart } from '../ag.generated';
import type { FolderTree } from '../../components/panel/right/FolderTree';
export type { Drive } from '../ag.generated';

interface FoldersState {
  drives: Drive[];
  /** Newest first, only folders that are there. */
  recent: string[];
  /** No disk could be listed: the centre says so, and a path can still be typed (Alt+D). */
  unavailable: boolean;
}

export const useModernFolders = create<FoldersState>(() => ({ drives: [], recent: [], unavailable: false }));

/** How the library writes an album in `currentFolderPath`. */
const ALBUM_PREFIX = 'Album: ';

/** The disks and recent folders, as the backend sees them now. None if they could not be listed. */
async function listDisks(lastFolder: string | null, expanded: string[]): Promise<ModernStart | null> {
  try {
    const start = await ag('modern_start', { lastFolder, expanded });
    useModernFolders.setState({ drives: start.drives, recent: start.recent, unavailable: start.drives.length === 0 });
    return start;
  } catch (err) {
    console.error('Could not list the disks:', err);
    useModernFolders.setState({ unavailable: true });
    return null;
  }
}

/** A disk's row before anything has been read from it. */
const unreadDisk = (path: string): FolderTree => ({
  name: folderName(path),
  path,
  children: [],
  isDir: true,
  hasSubdirs: true,
  unread: true,
});

/** A disk's row, with the folders left unfolded on it. One that cannot be read keeps its row. */
async function readDisk(path: string, expanded: string[]): Promise<FolderTree> {
  if (!expanded.includes(path)) return unreadDisk(path);
  try {
    const [tree] = await invoke<FolderTree[]>(Invokes.GetPinnedFolderTrees, {
      paths: [path],
      expandedFolders: expanded,
      showImageCounts: false,
    });
    return tree ?? unreadDisk(path);
  } catch (err) {
    console.error('Could not read a disk for the folder column:', err);
    return unreadDisk(path);
  }
}

/** Puts a disk's row back in the column, read again. */
function showDisk(tree: FolderTree) {
  useLibraryStore.getState().setLibrary((state: { folderTrees: FolderTree[] }) => ({
    folderTrees: state.folderTrees.map((row) => (row.path === tree.path ? tree : row)),
  }));
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
 * The Modern window's stand-in for Continue Session: fill the column, then
 * open the folder last open, with its photos read ahead if they are of that
 * folder. A photo opened with Argentum stays open; the folder fills in behind
 * it, and is the photo's own when the photo got there first. If no disk can be
 * listed nothing changes, and today's start screen shows.
 */
export function startModernLibrary(preloaded: { current: Preloaded | null }, selectFolder: SelectFolder): void {
  const run = async () => {
    const { appSettings } = useSettingsStore.getState();
    const { setLibrary } = useLibraryStore.getState();
    const saved = appSettings?.lastFolderState;
    const start = await listDisks(saved?.currentFolderPath ?? null, saved?.expandedFolders ?? []);
    const roots = start?.drives.map((d) => d.path) ?? [];
    if (!start || roots.length === 0) return;
    watchLibrary();

    setLibrary({
      rootPaths: roots,
      expandedFolders: new Set(start.expanded),
      folderTrees: roots.map(unreadDisk),
    });
    await Promise.all(roots.map((path) => readDisk(path, start.expanded).then(showDisk)));

    const opened = useLibraryStore.getState().currentFolderPath;
    const folder = opened ?? start.startFolder;
    if (!folder) return;
    const ahead = preloaded.current;
    let images: ImageFile[] | undefined;
    if (ahead?.currentPath === folder && ahead.images) {
      images = await ahead.images.catch(() => undefined);
      ahead.images = undefined;
    }
    const { initialFileToOpen, externalEditSession } = useProcessStore.getState();
    const keepEditor = !!(
      opened ||
      useEditorStore.getState().selectedImage ||
      initialFileToOpen ||
      externalEditSession
    );
    await selectFolder(folder, false, images, !!opened, keepEditor);
  };
  run().catch((err) => console.error('Failed to open the Modern folder column:', err));
}

/**
 * Read the column again: the disks (one plugged in since shows up) and the
 * favourites, without the library's photo counts; the rows count again
 * themselves (folderCounts.ts). Modern's stand-in for the library's own
 * refresh, which would count every folder on a disk.
 */
export async function refreshModernTrees(): Promise<void> {
  forgetFolderCounts();
  const { appSettings } = useSettingsStore.getState();
  const { expandedFolders, setLibrary } = useLibraryStore.getState();
  const expanded = Array.from(expandedFolders);
  const pinned = appSettings?.pinnedFolders ?? [];
  const start = await listDisks(null, []);
  if (!start || start.drives.length === 0) return;
  const roots = start.drives.map((d) => d.path);
  try {
    const [folderTrees, pinnedFolderTrees] = await Promise.all([
      Promise.all(roots.map((path) => readDisk(path, expanded))),
      pinned.length > 0
        ? invoke<unknown[]>(Invokes.GetPinnedFolderTrees, {
            paths: pinned,
            expandedFolders: expanded,
            showImageCounts: false,
          })
        : Promise.resolve([]),
    ]);
    setLibrary({ rootPaths: roots, folderTrees, pinnedFolderTrees });
  } catch (err) {
    console.error('Failed to refresh the folder column:', err);
  }
}

let watching = false;

/**
 * Every folder opened goes to the top of Recent, and the tree's roots stay the
 * disks. If the library's own refresh puts the folders added one by one back
 * in the tree, the disks are read back. A folder opened from outside the tree
 * unfolds the folders above it, which nothing has read yet, so its disk is
 * read for them.
 */
function watchLibrary() {
  if (watching) return;
  watching = true;
  useLibraryStore.subscribe((state, previous) => {
    const folder = state.currentFolderPath;
    if (folder && folder !== previous.currentFolderPath && !folder.startsWith(ALBUM_PREFIX)) {
      ag('add_recent_folder', { path: folder })
        .then((recent) => useModernFolders.setState({ recent }))
        .catch((err) => console.warn('Recent folders not saved:', err));
      const disk = rootContaining(folder, state.rootPaths);
      if (disk && state.expandedFolders.size > previous.expandedFolders.size) {
        readDisk(disk, Array.from(state.expandedFolders)).then(showDisk);
      }
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

/**
 * The tree with a folder's children in place, as they are now: opening a folder
 * also settles the arrow it had only guessed. RapidRAW's, from src/App.tsx.
 */
export function insertChildrenIntoTree(node: FolderTree, targetPath: string, newChildren: FolderTree[]): FolderTree {
  if (node.path === targetPath) {
    const children = newChildren.map((newChild) => {
      const existing = node.children?.find((child) => child.path === newChild.path);
      return existing?.children?.length ? { ...newChild, children: existing.children } : newChild;
    });
    return { ...node, children, hasSubdirs: children.length > 0, unread: false };
  }
  return { ...node, children: node.children?.map((child) => insertChildrenIntoTree(child, targetPath, newChildren)) };
}

/** A disk as the column names it: its letter or mount point, then its own name. */
export function driveName(drive: Drive): string {
  const where = drive.path.replace(/[\\/]+$/, '') || drive.path;
  return drive.label ? `${where}  ${drive.label}` : where;
}

/** The last part of a folder's path. */
export function folderName(path: string): string {
  const parts = path.split(/[\\/]+/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

/** The folder a photo is in. A drive's own folder is its root, E:\, not E:. */
export function folderOfPhoto(path: string): string {
  const folder = parentFolder(path);
  return /^[A-Za-z]:$/.test(folder) ? `${folder}\\` : folder || '/';
}

/**
 * Opens a photo handed in from outside, and takes the library to its folder:
 * the path over the photo, the filmstrip and the arrows are all of the folder
 * on show, which until now was whichever was open last.
 */
export function openFromOutside(path: string, openPhoto: (path: string) => void, showFolder: (folder: string) => void) {
  openPhoto(path);
  const folder = folderOfPhoto(path);
  if (folder !== useLibraryStore.getState().currentFolderPath) showFolder(folder);
}

/**
 * A folder's path as breadcrumbs, each with the path it opens: `E:\2024\Trip`
 * is E: (E:\), 2024 (E:\2024), Trip (E:\2024\Trip).
 */
export function crumbs(path: string): { name: string; path: string }[] {
  const sep = path.includes('\\') ? '\\' : '/';
  const parts = path.split(/[\\/]+/).filter(Boolean);
  const out: { name: string; path: string }[] = [];
  let acc = path.startsWith('\\\\') ? '\\\\' : path.startsWith('/') ? '/' : '';
  parts.forEach((part, i) => {
    acc = i === 0 ? `${acc}${part}` : `${acc}${acc.endsWith(sep) ? '' : sep}${part}`;
    // A drive letter's own folder is its root, E:\, not E:.
    const opens = i === 0 && /^[A-Za-z]:$/.test(part) ? `${part}${sep}` : acc;
    out.push({ name: part, path: opens });
  });
  return out;
}
