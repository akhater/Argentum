/**
 * The photo counts in the Modern folder column: how many photos each folder on
 * screen holds directly, not counting its subfolders. Ours.
 *
 * A row asks for its count when it shows, and the number arrives after the
 * folder has opened (mods/folder_counts.rs reads that one folder's listing).
 * Today's tree instead adds up every folder below before it shows any, which
 * on a whole disk is a scan of the disk. A few counts run at once; a row that
 * goes before its count started takes its request with it. Counts are kept
 * until the column is read again (refreshModernTrees), so folding and
 * unfolding costs nothing. A row nothing may look inside until it is opened
 * (mods/folder_walk.rs) asks for its count once it has been.
 */

import { useEffect } from 'react';
import { create } from 'zustand';
import { ag } from '../ag';

/** How many folders are counted at the same time. */
const AT_ONCE = 3;

interface FolderCountsState {
  /** Photos directly in each folder; absent until counted. */
  counts: Record<string, number>;
  /** Moves on each time the counts are dropped, so rows on screen ask again. */
  round: number;
}

const useFolderCounts = create<FolderCountsState>(() => ({ counts: {}, round: 0 }));

const waiting: string[] = [];
const asked = new Set<string>();
let running = 0;

function countNext() {
  while (running < AT_ONCE && waiting.length > 0) {
    const path = waiting.shift() as string;
    const round = useFolderCounts.getState().round;
    running += 1;
    ag('folder_photo_count', { path })
      .catch(() => 0)
      .then((count) => {
        if (useFolderCounts.getState().round !== round) return;
        useFolderCounts.setState((s) => ({ counts: { ...s.counts, [path]: count } }));
      })
      .finally(() => {
        running -= 1;
        countNext();
      });
  }
}

/** Drop every count, for when the column is read again. */
export function forgetFolderCounts() {
  waiting.length = 0;
  asked.clear();
  useFolderCounts.setState((s) => ({ counts: {}, round: s.round + 1 }));
}

/** Whether a row on screen may have its folder read for a count. */
export const rowWantsCount = (row: { unread?: boolean; open: boolean; selected: boolean }) =>
  !row.unread || row.open || row.selected;

/** The photos directly in `path`, once counted; asks for the count while `wanted`. */
export function useFolderCount(path: string, wanted: boolean): number | undefined {
  const round = useFolderCounts((s) => s.round);
  useEffect(() => {
    if (!wanted || asked.has(path)) return;
    asked.add(path);
    waiting.push(path);
    countNext();
    return () => {
      const at = waiting.indexOf(path);
      if (at >= 0) {
        waiting.splice(at, 1);
        asked.delete(path);
      }
    };
  }, [path, wanted, round]);
  return useFolderCounts((s) => (wanted ? s.counts[path] : undefined));
}
