// The only way the Browse view reaches the disk. The calls mirror the app's
// (src-tauri/src/mods/browse/mod.rs) and every type is generated from its
// contract, so the app and the browse fake are interchangeable.

import type { BrowseEvent, Folder, ListingId, Places } from './browse.generated';

export interface BrowseClient {
  /** Drives, Pictures and home, and the folders remembered. */
  places(): Promise<Places>;
  /** A folder's folders, by name. */
  folders(path: string): Promise<Folder[]>;
  /**
   * List a folder's photos. Refused at once with a BrowseError if it cannot
   * be read; else its id, and the photos follow as `listed` events. A newer
   * listing stops every older one.
   */
  list(path: string): Promise<ListingId>;
  /**
   * The thumbnails wanted, the most wanted first, in place of the last list.
   * Each comes as a `thumb` event, or `noThumb`.
   */
  want(paths: string[]): Promise<void>;
  /** A thumbnail announced by a `thumb` event, as an image; null once it has gone. */
  thumb(path: string): Promise<Blob | null>;
  /** Pin or unpin a folder; the favourites after. */
  setFavourite(path: string, on: boolean): Promise<string[]>;
  /** A folder opened; the recent folders after, the latest first. */
  addRecent(path: string): Promise<string[]>;
  /** Listing and thumbnail events. Returns the unsubscribe. */
  subscribe(listener: (event: BrowseEvent) => void): () => void;
}
