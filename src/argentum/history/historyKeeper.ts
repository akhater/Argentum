/**
 * Each photo keeps its History: in its sidecar, given back when it opens. Ours.
 *
 * Their editor starts a new History whenever a photo opens (resetHistory in
 * useAppNavigation.ts and useImageLoader.ts) and keeps none, so leaving a photo
 * lost its steps. This watches the editor's store, as historyOnRelease.ts does,
 * and no line of theirs changes:
 *
 * - When their code starts a History (one state, the edit itself), the one the
 *   photo's sidecar keeps goes in its place, at the step it was at, if it was
 *   written for that edit and replaying it lands there (historyRecord.ts).
 *   Otherwise theirs stays. Their reset and paste start one too, and the edit
 *   they leave is not the one the sidecar's History was written for.
 * - After a step, an undo or a redo, the History is written 600 ms after the
 *   last change: after their save of the edit (300 ms) and their step (500 ms),
 *   so the two never write the sidecar at once, and every change puts both off
 *   again. Leaving the photo writes it at once, after their own save, which
 *   leaving sends first.
 * - A change their 500 ms had not made a step yet when the photo is left is
 *   written as the step it would have been, as RAWmakase saves a drag still
 *   going (src/edit_session/history.rs @ bf5346d, `saved`).
 */

import debounce from 'lodash.debounce';
import { useEditorStore } from '../../store/useEditorStore';
import type { Adjustments } from '../../utils/adjustments';
import { ag } from '../ag';
import { trackSave } from '../savesInFlight';
import { historyNames, keepNames } from '../layout/historyNames';
import { compact, expand } from './historyRecord';

/** After their save of the edit (300 ms) and their step (500 ms). */
export const WRITE_AFTER_MS = 600;

/** The open photo's History as last changed. */
interface Shown {
  path: string;
  history: Adjustments[];
  index: number;
  adjustments: Adjustments;
}

/** The photo whose History is kept here: set once their code has started it. */
let keptFor: string | undefined;
/** The History last put in from a sidecar, so putting it in is not a change to write. */
let restored: Adjustments[] | null = null;
/** Reads and writes in the order asked, so a photo opened again reads what leaving it wrote. */
let queue: Promise<unknown> = Promise.resolve();

const inOrder = (task: () => Promise<unknown>) => {
  queue = trackSave(queue.then(task).catch((err) => console.error('History: could not keep it:', err)));
};

/** What `shown` keeps: its steps, or with a change not yet a step, the step it would have been. */
const toKeep = ({ history, index, adjustments }: Shown) => {
  const states = history[index] === adjustments ? history : [...history.slice(0, index + 1), adjustments];
  const end = history[index] === adjustments ? index : states.length - 1;
  return compact({ states, end, names: historyNames(states) });
};

const writeSoon = debounce((shown: Shown) => {
  const record = toKeep(shown);
  inOrder(() => ag('set_history', { path: shown.path, record }));
}, WRITE_AFTER_MS);

/** Writes the History now instead of 600 ms after the last change (closing the window). */
export const flushHistoryWrite = () => writeSoon.flush();

/** Put the photo's kept History in place of `fresh`, the one their code just started, if it still fits. */
const restore = (path: string, fresh: Adjustments[]) => {
  const current = fresh[0];
  inOrder(async () => {
    const record = await ag('history', { path });
    const now = useEditorStore.getState();
    // Anything since (an edit, another photo, another fresh start) and theirs stays.
    if (now.selectedImage?.path !== path || now.history !== fresh || now.adjustments !== current) return;
    const kept = record && expand(record, current);
    if (!kept) return;
    keepNames(kept.states, kept.names);
    restored = kept.states;
    useEditorStore.setState({ history: kept.states, historyIndex: kept.end });
  });
};

useEditorStore.subscribe((state, prev) => {
  const path = state.selectedImage?.path;
  if (path !== keptFor) {
    // Another photo, or none: what the last one changed is written now.
    writeSoon.flush();
    keptFor = undefined;
  }
  if (!path) return;

  const started =
    state.history !== prev.history && state.history.length === 1 && state.history[0] === state.adjustments;
  if (started) {
    // A fresh start (opening, a reset, a paste) replaces what was waiting to be written.
    writeSoon.cancel();
    keptFor = path;
    restore(path, state.history);
    return;
  }
  // Still opening: until their code starts its History, the store holds the last photo's.
  if (path !== keptFor) return;
  if (
    state.history === prev.history &&
    state.historyIndex === prev.historyIndex &&
    state.adjustments === prev.adjustments
  )
    return;
  if (state.history === restored && prev.history !== restored) return;
  writeSoon({ path, history: state.history, index: state.historyIndex, adjustments: state.adjustments });
});
