/**
 * Compact sliders, on or off. Ours.
 *
 * The layout is all in compactSliders.css, keyed on one attribute on <html>.
 * This reads the preference at startup, sets the attribute, and changes both
 * when the switch in Settings is flipped. The switch and the startup read are
 * in different places, so the state sits here rather than in either.
 *
 * Stored in argentum-processing.json beside our other preferences, through the
 * same `ag` commands, rather than in their settings.json.
 */

import { create } from 'zustand';
import { ag } from './ag';
import './compactSliders.css';

const ATTRIBUTE = 'data-ag-sliders';

function show(on: boolean) {
  if (on) {
    document.documentElement.setAttribute(ATTRIBUTE, 'compact');
  } else {
    document.documentElement.removeAttribute(ATTRIBUTE);
  }
}

interface CompactSlidersState {
  /** Null until the preference has been read. */
  on: boolean | null;
  load: () => Promise<void>;
  choose: (on: boolean) => Promise<void>;
}

export const useCompactSliders = create<CompactSlidersState>((set, get) => ({
  on: null,

  load: async () => {
    let on = false;
    try {
      on = await ag<boolean>('compact_sliders');
    } catch {
      // Unreadable is off, which is how the sliders were drawn before this.
    }
    set({ on });
    show(on);
  },

  choose: async (on) => {
    const previous = get().on ?? false;
    // Shown at once: a switch that lags a click reads as broken.
    set({ on });
    show(on);
    try {
      await ag('set_compact_sliders', { on });
    } catch {
      // Not saved, so not chosen: put back what will be there next start.
      set({ on: previous });
      show(previous);
    }
  },
}));
