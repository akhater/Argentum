/**
 * The chosen TIFF bit depth, where React can see it change. Ours.
 *
 * WHY THIS EXISTS
 *
 * `ExportPrecision` saves the depth to the Rust side and that is enough for the
 * export itself, which reads it when it runs. It is not enough for the size
 * estimate beside the export button: that number is produced by an effect in
 * their `ExportPanel`, and an effect only re-runs when something in its
 * dependency list changes. A preference written to a file on another thread is
 * not in any dependency list, so the estimate went on showing the size of the
 * depth you had before — 8-bit selected, 16-bit number, or the reverse.
 *
 * AK found that within a minute of first running it. It is exactly the kind of
 * thing the unit tests could not see: every layer was correct on its own, and
 * the number on screen was still wrong.
 *
 * So the depth is published here as well as saved, and their effect takes it as
 * a dependency. `useSyncExternalStore` rather than a React context because the
 * writer and the reader are in different trees — the control is portalled into
 * their panel from `Argentum.tsx`, so there is no common provider to hang a
 * context on without adding one to a file of theirs.
 *
 * This is the second line Argentum spends in `ExportPanel.tsx`, and it buys the
 * estimate telling the truth. There is no cheaper version: `estimatedSize` is
 * `useState` inside their component and the debounced estimator is a `useMemo`
 * beside it, so nothing outside can trigger a re-estimate.
 */

import { useSyncExternalStore } from 'react';

export type TiffDepth = 8 | 16;

/** What an export did before the control existed, and the default still. */
let current: TiffDepth = 16;

const listeners = new Set<() => void>();

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function getSnapshot(): TiffDepth {
  return current;
}

/**
 * Record the depth now in force.
 *
 * Called by `ExportPrecision` when it reads the stored preference and whenever
 * the user picks a different one. Notifying on an unchanged value would re-run
 * their estimate effect for nothing, so it returns early.
 */
export function publishTiffDepth(depth: TiffDepth): void {
  if (depth === current) {
    return;
  }
  current = depth;
  for (const listener of listeners) {
    listener();
  }
}

/** The depth, as a value React will re-render and re-run effects for. */
export function useTiffDepth(): TiffDepth {
  return useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
}
