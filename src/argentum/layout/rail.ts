/**
 * What the RAWmakase arrangement's rail switches between. Ours.
 *
 * RAWmakase's rail (src/app/side_tabs.rs) first switches the adjustments panel
 * between Edit and the tool drawers that take its place, then, after a gap,
 * switches the tab panel beside it. Here the tools and tabs are today's own
 * panels, with today's icons and names (PanelSwitcher.tsx), so a panel added
 * there is one line here.
 */

import { History, type LucideIcon } from 'lucide-react';
import { Panel } from '../../components/ui/AppProperties';
import { PANEL_ICONS, PANEL_TITLES } from '../../components/panel/PanelSwitcher';
import type { AgKey } from '../locales';

/**
 * The tools, top to bottom: Edit (the adjustments), then the panels that take
 * their place while in use, as RAWmakase's drawers do (inspector.rs
 * DRAWER_TOOLS). Each puts the editor in its own mode through today's
 * activePanel.
 */
export const RAIL_TOOLS: readonly Panel[] = [Panel.Adjustments, Panel.Crop, Panel.Masks, Panel.Ai];

/** A tab of the panel beside the adjustments: a panel of today's, or the History list. */
export type SideTabId = Panel.Presets | Panel.Metadata | Panel.Export | Panel.Tethering | 'history';

/** A tooltip: today's own for one of today's panels (PANEL_TITLES), or one of ours. */
export type RailTitle = { theirs: string } | { ours: AgKey };

export interface SideTab {
  id: SideTabId;
  icon: LucideIcon;
  title: RailTitle;
}

const panelTab = (panel: SideTabId & Panel): SideTab => ({
  id: panel,
  icon: PANEL_ICONS[panel],
  title: { theirs: PANEL_TITLES[panel] },
});

/**
 * The tabs, after the tools: RAWmakase's Presets and History (side_tabs.rs
 * TABS), then the panels today's left side held besides the folders. Tethering
 * shows only where today's workspace has it, which is where it is supported.
 */
export const SIDE_TABS: readonly SideTab[] = [
  panelTab(Panel.Presets),
  { id: 'history', icon: History, title: { ours: 'layoutHistory' } },
  panelTab(Panel.Metadata),
  panelTab(Panel.Export),
  panelTab(Panel.Tethering),
];

/** The tool column shows this panel: today's activePanel if it is a tool, otherwise Edit. */
export function toolShown(activePanel: Panel | null): Panel {
  return activePanel && RAIL_TOOLS.includes(activePanel) ? activePanel : Panel.Adjustments;
}
