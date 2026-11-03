/**
 * Focus points: the switch, and what each photo's file says. Ours.
 *
 * One switch for every photo, kept across restarts in argentum-processing.json
 * (`focus_points_shown`, mods/focus_points.rs). The top bar's button and
 * Shift+F flip it; the overlay over the photo (FocusOverlay.tsx) reads it.
 * Turning it on over a photo with nothing to draw says so in a toast, as
 * Shoebox's viewer does, since the photo itself does not change.
 */

import { useEffect, useState } from 'react';
import { create } from 'zustand';
import { toast } from 'react-toastify';
import { ag } from './ag';
import type { Focus } from './ag.generated';
import { agText } from './locales';
import { hasFocusPoints } from './FocusPoints';
import { useEditorStore } from '../store/useEditorStore';

/** Per path, for the session: a file's metadata does not change while the app runs. */
const asked = new Map<string, Promise<Focus | null>>();

/** What `focus_points` answers for `path`, asked once per session. */
export const askFocus = (path: string): Promise<Focus | null> => {
  let answer = asked.get(path);
  if (!answer) {
    answer = ag('focus_points', { path });
    asked.set(path, answer);
    answer.catch(() => asked.delete(path));
  }
  return answer;
};

/**
 * The open photo's focus while `on`: `undefined` until its file is read (or
 * off), `null` when it records nothing. Read only once the photo is ready:
 * until then the editor may still hold the last photo's edit.
 */
export function usePhotoFocus(on: boolean): Focus | null | undefined {
  const path = useEditorStore((s) => (s.selectedImage?.isReady ? s.selectedImage.path : null));
  const [read, setRead] = useState<{ path: string; focus: Focus | null } | null>(null);
  useEffect(() => {
    if (!on || !path) return;
    let current = true;
    askFocus(path)
      .catch(() => null)
      .then((focus) => current && setRead({ path, focus }));
    return () => {
      current = false;
    };
  }, [on, path]);
  return on && read && read.path === path ? read.focus : undefined;
}

/** The toast's words for a photo with nothing to draw. */
export const unrecordedText = (focus: Focus | null) =>
  agText(focus?.mode === 'Manual focus' ? 'focusPointsManualToast' : 'focusPointsUnrecordedToast');

interface FocusPointsState {
  on: boolean;
  /** Read the switch as it was left. */
  load: () => Promise<void>;
  toggle: () => void;
}

export const useFocusPoints = create<FocusPointsState>((set, get) => ({
  on: false,

  load: async () => {
    // Unreadable is off.
    set({ on: await ag('focus_points_shown').catch(() => false) });
  },

  toggle: () => {
    const on = !get().on;
    set({ on });
    // A failed save leaves it as switched for this session.
    ag('set_focus_points_shown', { on }).catch(() => {});
    const path = useEditorStore.getState().selectedImage?.path;
    if (on && path) {
      askFocus(path)
        .then((focus) => {
          if (!hasFocusPoints(focus)) toast.info(unrecordedText(focus), { autoClose: 1800 });
        })
        .catch(() => {});
    }
  },
}));
