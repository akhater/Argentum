/**
 * Compact panels, on or off, and how much space between rows. Ours.
 * (Compact sliders until the tone curve joined.)
 *
 * The layout is all in compactSliders.css, keyed on one attribute on <html>,
 * and the row spacing is one CSS variable beside it. This reads both
 * preferences at startup, puts them on the page, and changes them when the
 * controls in Settings move. The controls and the startup read are in
 * different places, so the state sits here rather than in either.
 *
 * Stored in argentum-processing.json beside our other preferences, through the
 * same `ag` commands, rather than in their settings.json.
 */

import { create } from 'zustand';
import { ag } from './ag';
import './compactSliders.css';

const ATTRIBUTE = 'data-ag-sliders';
const GAP_VARIABLE = '--ag-slider-gap';

/** Space under each row, in pixels, until chosen: 20px a row, Lightroom's. */
export const DEFAULT_GAP = 4;
/** The slider's end. Rust clamps to the same. */
export const MAX_GAP = 12;

function show(on: boolean) {
  if (on) {
    document.documentElement.setAttribute(ATTRIBUTE, 'compact');
  } else {
    document.documentElement.removeAttribute(ATTRIBUTE);
  }
}

function showGap(gap: number) {
  document.documentElement.style.setProperty(GAP_VARIABLE, `${gap}px`);
}

/**
 * Row spacing is saved once the slider settles, not on every step of a drag:
 * each save is a read and a write of the preferences file.
 */
let gapSave: ReturnType<typeof setTimeout> | undefined;

interface CompactSlidersState {
  /** Null until the preference has been read. */
  on: boolean | null;
  gap: number;
  load: () => Promise<void>;
  choose: (on: boolean) => Promise<void>;
  chooseGap: (gap: number) => void;
}

export const useCompactSliders = create<CompactSlidersState>((set, get) => ({
  on: null,
  gap: DEFAULT_GAP,

  load: async () => {
    // Unreadable is off, which is how the sliders were drawn before this, and
    // the default spacing.
    const [on, gap] = await Promise.all([
      ag<boolean>('compact_sliders').catch(() => false),
      ag<number>('compact_slider_gap').catch(() => DEFAULT_GAP),
    ]);
    set({ on, gap });
    showGap(gap);
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

  chooseGap: (gap) => {
    const clamped = Math.max(0, Math.min(MAX_GAP, Math.round(gap)));
    // Live, so the panels move as the slider does.
    set({ gap: clamped });
    showGap(clamped);
    clearTimeout(gapSave);
    gapSave = setTimeout(() => {
      // A failed save leaves the spacing as chosen for this session; snapping
      // it back mid-drag would be worse than the next start forgetting it.
      ag('set_compact_slider_gap', { gap: clamped }).catch(() => {});
    }, 400);
  },
}));
