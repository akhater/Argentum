/**
 * Which arrangement of today's panels the editor shows, and how the parts only
 * the RAWmakase arrangement has were left. Ours.
 *
 * Classic is today's arrangement and the default. RAWmakase arranges the same
 * panels as RAWmakase 0.3 does; see RawmakaseEditor.tsx. Chosen in Settings >
 * General > Layout, read once at startup, and stored in
 * argentum-processing.json beside Compact panels through the same `ag`
 * commands (mods/editor_layout.rs), not in their settings.json.
 *
 * Everything else the arrangement remembers is today's own state, saved where
 * it always was: panel widths and folds (useUIStore's workspace and
 * uiVisibility), the adjustment sections' folds, and whether the histogram is
 * showing (isWaveformVisible, which also has its keyboard shortcut).
 */

import { create } from 'zustand';
import { Panel } from '../../components/ui/AppProperties';
import { ag } from '../ag';
import type { SideTabId } from './rail';

/** Mirrors mods::editor_layout::EditorLayout. Hand-written: the app does not generate its types yet. */
export type EditorLayoutName = 'classic' | 'rawmakase';

/** Mirrors mods::editor_layout::LayoutPanels. Hand-written, as above. */
export interface LayoutPanels {
  /** The Navigator section is open. */
  navigatorOpen: boolean;
  /** The tab open beside the adjustments, or null while the tab panel is folded. */
  tab: SideTabId | null;
  tabWidthPx: number;
}

/** As RAWmakase opens (side_tabs.rs): the Navigator showing, Presets beside the adjustments at 245 px. */
export const DEFAULT_PANELS: LayoutPanels = { navigatorOpen: true, tab: Panel.Presets, tabWidthPx: 245 };
/** How far the tab panel drags: workspace.rs PRESETS_MIN, and the max_size side_tabs.rs gives it. */
export const MIN_TAB_WIDTH_PX = 224;
export const MAX_TAB_WIDTH_PX = 400;

/**
 * Panels are saved once a change settles, not on every step of a drag: each
 * save is a read and a write of the preferences file.
 */
let panelsSave: ReturnType<typeof setTimeout> | undefined;

interface EditorLayoutState {
  /** Null until the preference has been read. */
  layout: EditorLayoutName | null;
  panels: LayoutPanels;
  load: () => Promise<void>;
  choose: (layout: EditorLayoutName) => Promise<void>;
  changePanels: (change: Partial<LayoutPanels>) => void;
}

export const useEditorLayout = create<EditorLayoutState>((set, get) => ({
  layout: null,
  panels: DEFAULT_PANELS,

  load: async () => {
    // Unreadable is Classic, today's arrangement, and RAWmakase's panels as it opens them.
    const [layout, panels] = await Promise.all([
      ag<EditorLayoutName>('editor_layout').catch((): EditorLayoutName => 'classic'),
      ag<LayoutPanels | null>('editor_layout_panels').catch(() => null),
    ]);
    set({ layout, panels: panels ?? DEFAULT_PANELS });
  },

  choose: async (layout) => {
    const previous = get().layout ?? 'classic';
    // Shown at once: a choice that lags a click reads as broken.
    set({ layout });
    try {
      await ag('set_editor_layout', { layout });
    } catch {
      // Not saved, so not chosen: put back what will be there next start.
      set({ layout: previous });
    }
  },

  changePanels: (change) => {
    const panels = { ...get().panels, ...change };
    set({ panels });
    clearTimeout(panelsSave);
    panelsSave = setTimeout(() => {
      // A failed save leaves the panels as left for this session; the next
      // start opening them as before is the whole cost.
      ag('set_editor_layout_panels', { panels }).catch(() => {});
    }, 400);
  },
}));

/** Whether the RAWmakase arrangement is chosen. */
export const useRawmakaseLayout = () => useEditorLayout((s) => s.layout === 'rawmakase');
