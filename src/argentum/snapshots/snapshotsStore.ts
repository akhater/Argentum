/**
 * Each photo's snapshots, as its sidecar keeps them (mods/snapshots.rs). Ours.
 *
 * Read when a photo is shown and written whole on every change: a photo has a
 * handful, so the list is small, and writing it whole keeps the sidecar the
 * one place they live.
 */

import { create } from 'zustand';
import { ag } from '../ag';
import type { Snapshot } from '../ag.generated';
export type { Snapshot } from '../ag.generated';

interface SnapshotsState {
  byPath: Record<string, Snapshot[]>;
  /** Read a photo's snapshots from its sidecar. */
  load: (path: string) => Promise<void>;
  /** Keep `list` as the photo's snapshots: shown at once, then written. Only after they were read. */
  keep: (path: string, list: Snapshot[]) => void;
}

/** The last save of each photo, which the next one waits for. */
const saving = new Map<string, Promise<void>>();

export const useSnapshots = create<SnapshotsState>((set, get) => ({
  byPath: {},

  load: async (path) => {
    try {
      const list = await ag('snapshots', { path });
      set((s) => ({ byPath: { ...s.byPath, [path]: list } }));
    } catch (err) {
      console.error('Snapshots: could not read them:', err);
    }
  },

  keep: (path, list) => {
    // Never read (it failed, or has not answered yet): writing now would put
    // `list` over snapshots the sidecar still holds.
    if (!(path in get().byPath)) {
      console.error("Snapshots: not kept, the photo's own were never read:", path);
      return;
    }
    const was = get().byPath[path];
    set((s) => ({ byPath: { ...s.byPath, [path]: list } }));
    // One save at a time per photo, in the order asked: two saves in flight
    // could land the older list last.
    const save = (saving.get(path) ?? Promise.resolve())
      .then(() => ag('set_snapshots', { path, snapshots: list }))
      .then(
        () => undefined,
        (err) => {
          console.error('Snapshots: could not keep them:', err);
          set((s) => ({ byPath: { ...s.byPath, [path]: was } }));
        },
      );
    saving.set(path, save);
  },
}));
