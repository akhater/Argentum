// Browsing inside the app: the same BrowseClient as the browse fake, over
// Argentum's one `ag` command (src-tauri/src/mods/dispatch.rs) and the
// `ag-browse` event (src-tauri/src/mods/browse/mod.rs). A thumbnail crosses
// as the JPEG's bytes; a refused call rejects with the app's BrowseError.

import { subscription, tauriTransport, type Transport } from '../engine/tauri';
import type { BrowseEvent } from './browse.generated';
import type { BrowseClient } from './client';

/** The Tauri event browse events arrive as. */
export const BROWSE_EVENT = 'ag-browse';

export function createTauriBrowse(transport: Transport = tauriTransport): BrowseClient {
  const call = transport.ag;
  return {
    places: () => call('browse_places'),
    folders: (path) => call('browse_folders', { path }),
    list: (path) => call('browse_list', { path }),
    want: (paths) => call('browse_want', { paths }),
    async thumb(path) {
      const bytes = await call<ArrayBuffer | null>('browse_thumb', { path });
      return bytes ? new Blob([bytes], { type: 'image/jpeg' }) : null;
    },
    setFavourite: (path, on) => call('browse_favourite', { path, on }),
    addRecent: (path) => call('browse_add_recent', { path }),
    subscribe: (listener) => subscription<BrowseEvent>(transport, BROWSE_EVENT, listener),
  };
}
