/**
 * "Still rendering" indicator. Ours.
 *
 * RapidRAW has a spinner in the toolbar, but it is driven by `isViewLoading` —
 * that is *opening a photo*, not re-rendering the preview after a slider moves.
 * There was no signal for the thing you actually wait for, which matters most
 * when judging colour: a reading taken mid-render is a reading of the old frame.
 *
 * No changes to their files were needed. The pipeline already emits
 * `wgpu-frame-ready` when the native render completes (lib.rs), and adjustments
 * live in a store we can subscribe to. Busy is simply "adjustments changed and
 * no frame has arrived since".
 *
 * Shown only after a short delay. Most renders finish in tens of milliseconds
 * and a spinner that flashes on every slider tick is worse than none.
 */

import { useEffect, useRef, useState } from 'react';
import { Loader2 } from 'lucide-react';
import { listen } from '@tauri-apps/api/event';
import { useEditorStore } from '../store/useEditorStore';

/** Below this, a render is imperceptible and the spinner is just noise. */
const SHOW_AFTER_MS = 140;

export default function RenderStatus() {
  const [busy, setBusy] = useState(false);
  const timer = useRef<number | null>(null);

  useEffect(() => {
    const clear = () => {
      if (timer.current !== null) {
        window.clearTimeout(timer.current);
        timer.current = null;
      }
    };

    // Any change to the adjustments means a new frame is on its way.
    const unsubscribe = useEditorStore.subscribe((state: any, prev: any) => {
      if (state.adjustments === prev.adjustments) {
        return;
      }
      if (timer.current === null) {
        timer.current = window.setTimeout(() => {
          timer.current = null;
          setBusy(true);
        }, SHOW_AFTER_MS);
      }
    });

    const unlisten = listen('wgpu-frame-ready', () => {
      clear();
      setBusy(false);
    });

    return () => {
      clear();
      unsubscribe();
      unlisten.then((f) => f()).catch(() => {});
    };
  }, []);

  if (!busy) {
    return null;
  }

  return (
    <div
      className="flex items-center px-1.5 text-text-secondary select-none"
      data-tooltip="Preview still rendering"
    >
      <Loader2 size={14} className="animate-spin" />
    </div>
  );
}
