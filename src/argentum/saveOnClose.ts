// Closing the window used to exit at once, taking what the timers still held: the open photo's last
// change (50 ms idle timer + 300 ms save), the last History step (500 ms, written 600 ms later) and
// the last preset change (500 ms). Tauri holds the window open while a close-requested listener
// runs and destroys it when the listener returns, so the listener flushes them all and waits for
// the writes. Only the "main" window exists (capabilities/default.json), and it holds the editor.
import { useEffect } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { debouncedSetHistory } from './historyOnRelease';
import { flushHistoryWrite } from './history/historyKeeper';
import { flushPendingSave, type QueuedEdit } from './pendingSave';
import { savesSettled } from './savesInFlight';
import { savePresetsSoon } from '../hooks/usePresets';

/**
 * The longest the window waits for the writes. A sidecar write takes milliseconds; two seconds
 * covers a slow or busy disk and still never feels like a window that will not close.
 */
export const CLOSE_WAIT_MS = 2000;

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
