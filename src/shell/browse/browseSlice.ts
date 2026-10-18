// The Browse view's part of the store: the folder tree, the folder shown and
// its photos, their thumbnails, and which of Browse and Edit is on screen.
// The disk is reached only through a BrowseClient (client.ts). Nothing here
// is saved but what the app remembers itself: favourite and recent folders.

import { ancestorsOf, samePath } from '../paths';
import type { Slice } from '../store';
import type { BrowseError, BrowseEvent, Folder, ListingId, Photo, Places } from './browse.generated';
import type { BrowseClient } from './client';
import { ThumbCache } from './thumbs';

export type Mode = 'browse' | 'edit';
export type SortBy = 'name' | 'date';

export interface BrowseState {
  mode: Mode;
  /** Drives, standard folders, favourites and recent folders; null until read. */
  places: Places | null;
  /** The folders in each folder the tree has opened, by path. */
  subfolders: Readonly<Record<string, readonly Folder[]>>;
  /** Folders open in the tree. */
  expanded: Readonly<Record<string, boolean>>;
  /** The folder shown; null at the start, which shows the recent folders. */
  folder: string | null;
  /** The folder's photos so far, sorted. The filmstrip shows the same (`photos`). */
  folderPhotos: readonly Photo[];
  /** All of the folder's photos have come. */
  listed: boolean;
  sortBy: SortBy;
  /** The photo picked in the grid. */
  selected: string | null;
  /** Bumped at each thumbnail that arrives or fails, so cells look again. */
  thumbsSeen: number;
  /** The last thing browsing could not do. */
  browseError: BrowseError | null;
}

export interface BrowseActions {
  /** Hear browse events and read the places. */
  initBrowse(): Promise<void>;
  disposeBrowse(): void;
  /** Browse or Edit. Edit shows the photo picked in the grid. */
  setMode(mode: Mode): void;
  /** Show a folder's photos, opened in the tree, and remember it as recent. */
  browseFolder(path: string): Promise<void>;
  /** Open or close a folder in the tree, reading its folders the first time. */
  toggleExpanded(path: string): Promise<void>;
  setSortBy(sortBy: SortBy): void;
  select(path: string | null): void;
  /** Open a photo in Edit; the filmstrip is the folder's photos. */
  edit(path: string): Promise<void>;
  /** The thumbnails wanted now, the most wanted first, instead of the last ones asked for. */
  wantThumbs(paths: readonly string[]): void;
  /** A thumbnail's URL, while the page holds it. */
  thumbUrl(path: string): string | undefined;
  /** No thumbnail can be made for this photo. */
  thumbFailed(path: string): boolean;
  setFavourite(path: string, on: boolean): Promise<void>;
  receiveBrowse(event: BrowseEvent): void;
}

const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' });

/** By name as people count (IMG_2 before IMG_10), or by date, oldest first. */
export function sortPhotos(photos: readonly Photo[], by: SortBy): Photo[] {
  const byName = (a: Photo, b: Photo) => collator.compare(a.name, b.name);
  return [...photos].sort(by === 'name' ? byName : (a, b) => a.modifiedMs - b.modifiedMs || byName(a, b));
}

/** What was thrown, as a BrowseError: the app's own, or anything else as `internal`. */
function asBrowseError(error: unknown): BrowseError {
  if (typeof error === 'object' && error !== null && 'kind' in error && 'message' in error) {
    return { kind: (error as BrowseError).kind, message: String(error.message) };
  }
  return { kind: 'internal', message: error instanceof Error ? error.message : String(error) };
}

/** Whether `path` is `root` or inside it. */
function isWithin(path: string, root: string): boolean {
  if (samePath(path, root)) return true;
  const sep = root.endsWith('\\') || root.endsWith('/') ? '' : root.includes('\\') ? '\\' : '/';
  return samePath(path.slice(0, root.length + sep.length), root + sep);
}

/**
 * The slice, on `browse`. Without one (the shell's Edit-only tests) it
 * browses nothing. `mode` is where the window starts.
 */
export const createBrowseSlice =
  (browse: BrowseClient | undefined, mode: Mode, cache = new ThumbCache()): Slice<BrowseState & BrowseActions> =>
  (set, get) => {
    let unsubscribe: (() => void) | null = null;
    // The listing shown, once its id is known; parts that come before it are
    // held by id, as the app may send them before it answers.
    let listing: ListingId | null = null;
    let listingsAsked = 0;
    const early = new Map<ListingId, { photos: Photo[]; done: boolean }[]>();
    let incoming: Photo[] = [];
    const failed = new Set<string>();
    let lastWanted: readonly string[] = [];

    const failBrowse = (e: unknown) => set({ browseError: asBrowseError(e) });

    /** The folder's photos so far, sorted, in the grid and the filmstrip. */
    const show = () => {
      const folderPhotos = sortPhotos(incoming, get().sortBy);
      set({ folderPhotos, photos: folderPhotos.map((p) => p.path) });
    };

    const takePart = (photos: Photo[], done: boolean) => {
      incoming = incoming.concat(photos);
      show();
      if (done) set({ listed: true });
    };

    const loadSubfolders = async (path: string) => {
      if (!browse || get().subfolders[path]) return;
      const found = await browse.folders(path);
      set({ subfolders: { ...get().subfolders, [path]: found } });
    };

    /** Open the tree down to `path`, under the deepest root that holds it. */
    const reveal = async (path: string) => {
      const places = get().places;
      if (!places) return;
      const root = [...places.standard, ...places.drives]
        .map((f) => f.path)
        .filter((r) => isWithin(path, r))
        .sort((a, b) => b.length - a.length)[0];
      if (!root) return;
      const chain = ancestorsOf(path).filter((a) => a.length >= root.length && isWithin(a, root));
      for (const at of chain) {
        await loadSubfolders(at);
        set({ expanded: { ...get().expanded, [at]: true } });
      }
    };

    return {
      mode,
      places: null,
      subfolders: {},
      expanded: {},
      folder: null,
      folderPhotos: [],
      listed: false,
      sortBy: 'name',
      selected: null,
      thumbsSeen: 0,
      browseError: null,

      async initBrowse() {
        if (!browse) return;
        unsubscribe ??= browse.subscribe((event) => get().receiveBrowse(event));
        try {
          set({ places: await browse.places() });
        } catch (e) {
          failBrowse(e);
        }
      },

      disposeBrowse() {
        unsubscribe?.();
        unsubscribe = null;
      },

      setMode(next) {
        set({ mode: next });
        const { selected, path } = get();
        if (next === 'edit' && selected && !(path && samePath(path, selected))) void get().openPhoto(selected);
      },

      async browseFolder(path) {
        if (!browse) return;
        const mine = ++listingsAsked;
        listing = null;
        early.clear();
        incoming = [];
        set({ folder: path, listed: false, selected: null, browseError: null });
        show();
        void reveal(path).catch(failBrowse);
        void browse
          .addRecent(path)
          .then((recent) => {
            const places = get().places;
            if (places) set({ places: { ...places, recent } });
          })
          .catch(failBrowse);
        try {
          const id = await browse.list(path);
          if (mine !== listingsAsked) return;
          listing = id;
          for (const part of early.get(id) ?? []) takePart(part.photos, part.done);
          early.clear();
        } catch (e) {
          if (mine === listingsAsked) {
            failBrowse(e);
            set({ listed: true });
          }
        }
      },

      async toggleExpanded(path) {
        if (get().expanded[path]) {
          set({ expanded: { ...get().expanded, [path]: false } });
          return;
        }
        try {
          await loadSubfolders(path);
          set({ expanded: { ...get().expanded, [path]: true } });
        } catch (e) {
          failBrowse(e);
        }
      },

      setSortBy(sortBy) {
        set({ sortBy });
        show();
      },

      select: (selected) => set({ selected }),

      async edit(path) {
        set({ selected: path, mode: 'edit' });
        const open = get().path;
        if (!(open && samePath(open, path))) await get().openPhoto(path);
      },

      wantThumbs(paths) {
        if (!browse) return;
        cache.touch(paths);
        const need = paths.filter((p) => !cache.has(p) && !failed.has(p));
        if (need.length === lastWanted.length && need.every((p, i) => p === lastWanted[i])) return;
        lastWanted = need;
        browse.want(need).catch(failBrowse);
      },

      thumbUrl: (path) => cache.url(path),
      thumbFailed: (path) => failed.has(path),

      async setFavourite(path, on) {
        if (!browse) return;
        try {
          const favourites = await browse.setFavourite(path, on);
          const places = get().places;
          if (places) set({ places: { ...places, favourites } });
        } catch (e) {
          failBrowse(e);
        }
      },

      receiveBrowse(event) {
        switch (event.event) {
          case 'listed':
            if (listing === null) {
              const held = early.get(event.listing) ?? [];
              early.set(event.listing, [...held, { photos: event.photos, done: event.done }]);
            } else if (event.listing === listing) {
              takePart(event.photos, event.done);
            }
            return;
          case 'thumb': {
            const { path } = event;
            if (!browse || cache.has(path)) return;
            browse
              .thumb(path)
              .then((blob) => {
                if (!blob) return;
                cache.put(path, blob);
                set({ thumbsSeen: get().thumbsSeen + 1 });
              })
              .catch(failBrowse);
            return;
          }
          case 'noThumb':
            failed.add(event.path);
            set({ thumbsSeen: get().thumbsSeen + 1 });
            return;
        }
      },
    };
  };
