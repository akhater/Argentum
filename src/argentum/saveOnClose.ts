// Closing the window would exit at once, taking what the timers still hold: the open photo's last
// change (EDIT_QUEUE_AFTER_MS idle timer + EDIT_SAVE_AFTER_MS save), the last History step
// (HISTORY_STEP_AFTER_MS, written HISTORY_WRITE_AFTER_MS later) and the last preset change
// (PRESETS_SAVE_AFTER_MS). Tauri holds the window open while a close-requested listener
// runs and destroys it when the listener returns, so the listener flushes them all and waits for
// the writes. Only the "main" window exists (capabilities/default.json), and it holds the editor.
import { useEffect } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { debouncedSetHistory } from './historyOnRelease';
import { flushHistoryWrite } from './history/historyKeeper';
import { flushPendingSave, type QueuedEdit } from './pendingSave';
import { savesSettled } from './savesInFlight';
import { CLOSE_WAIT_MS } from './editTimings';
import { savePresetsSoon } from '../hooks/usePresets';

/** Flushes what is pending and waits for the writes, at most `waitMs`. Never throws. */
export async function settlePendingWrites(queued: { current: QueuedEdit | null }, waitMs = CLOSE_WAIT_MS) {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    flushPendingSave(queued);
    debouncedSetHistory.flush();
    flushHistoryWrite();
    savePresetsSoon.flush();
    const bound = new Promise<void>((resolve) => {
      timer = setTimeout(resolve, waitMs);
    });
    await Promise.race([savesSettled(), bound]);
  } catch (err) {
    console.error('Saving before close failed:', err);
  } finally {
    clearTimeout(timer);
  }
}

/** Registers the close listener; resolves to the function that removes it. */
export const listenForClose = (queued: { current: QueuedEdit | null }) =>
  getCurrentWindow().onCloseRequested(() => settlePendingWrites(queued));

/** Mount once, where the open photo's queued edit lives. */
export function useSaveOnClose(queued: { current: QueuedEdit | null }) {
  useEffect(() => {
    const listening = listenForClose(queued);
    return () => {
      listening.then((stop) => stop()).catch(() => {});
    };
  }, [queued]);
}
