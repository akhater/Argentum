/**
 * The slim rail at the right edge of the RAWmakase arrangement. Ours.
 *
 * RAWmakase's (src/app/side_tabs.rs): 36 px wide, 28 px buttons 36 px apart,
 * the open one filled and marked on the window's edge. The tools first, then a
 * gap, then the tabs of the panel beside the adjustments.
 *
 * Edit shows the adjustments; pressed again it folds them. Another tool takes
 * their place and puts the editor in its mode (crop, masks, AI), and pressed
 * again goes back to Edit. Tools go through today's setPanel, so the editor,
 * its shortcuts and the Classic arrangement all agree on which is open; the
 * column folding is today's uiVisibility.rightPanel. A tab opens the panel
 * beside the adjustments, and its button pressed again folds it.
 */

import { useTranslation } from 'react-i18next';
import clsx from 'clsx';
import type { LucideIcon } from 'lucide-react';
import { Panel } from '../../components/ui/AppProperties';
import { PANEL_ICONS, PANEL_TITLES } from '../../components/panel/PanelSwitcher';
import { useUIStore } from '../../store/useUIStore';
import { useAgTranslation } from '../locales';
import { RAIL_TOOLS, type RailTitle, type SideTab, type SideTabId, toolShown } from './rail';

interface RailButtonProps {
  icon: LucideIcon;
  tooltip: string;
  isOn: boolean;
  onClick: () => void;
}

function RailButton({ icon: Icon, tooltip, isOn, onClick }: RailButtonProps) {
  return (
    <button
      className={clsx(
        'relative w-7 h-7 shrink-0 rounded-md flex items-center justify-center transition-colors',
        isOn ? 'bg-surface text-text-primary' : 'text-text-secondary hover:bg-surface hover:text-text-primary',
      )}
      onClick={onClick}
      data-tooltip={tooltip}
    >
      <Icon size={16} />
      {isOn && <span className="absolute -right-1 top-[7px] bottom-[7px] w-0.5 rounded-full bg-accent" />}
    </button>
  );
}

interface ToolRailProps {
  tabs: readonly SideTab[];
  /** The tab showing beside the adjustments, or null while folded. */
  tabShown: SideTabId | null;
  onTab: (tab: SideTabId) => void;
}

export default function ToolRail({ tabs, tabShown, onTab }: ToolRailProps) {
  const { t } = useTranslation();
  const agT = useAgTranslation();
  // PANEL_TITLES holds their keys typed as plain strings, so their typed t
  // takes them only cast; they are their own keys, used as PanelSwitcher uses them.
  const tooltip = (title: RailTitle) => ('ours' in title ? agT(title.ours) : (t(title.theirs as never) as string));
  const activePanel = useUIStore((s) => s.activePanel);
  const columnShown = useUIStore((s) => s.uiVisibility.rightPanel);
  const setPanel = useUIStore((s) => s.setPanel);
  const setUI = useUIStore((s) => s.setUI);
  const tool = toolShown(activePanel);

  const showColumn = (shown: boolean) => setUI((s) => ({ uiVisibility: { ...s.uiVisibility, rightPanel: shown } }));

  const pressTool = (panel: Panel) => {
    if (panel === Panel.Adjustments && tool === Panel.Adjustments) {
      showColumn(!columnShown);
      return;
    }
    // A tool pressed while it is open goes back to Edit, as RAWmakase closes a drawer.
    setPanel(tool === panel && columnShown ? Panel.Adjustments : panel);
    showColumn(true);
  };

  return (
    <div className="w-9 shrink-0 h-full bg-bg-secondary rounded-lg border border-surface flex flex-col items-center gap-2 py-3">
      {RAIL_TOOLS.map((panel) => (
        <RailButton
          key={panel}
          icon={PANEL_ICONS[panel]}
          tooltip={tooltip({ theirs: PANEL_TITLES[panel] })}
          isOn={tool === panel && columnShown}
          onClick={() => pressTool(panel)}
        />
      ))}
      <div className="h-6 shrink-0" />
      {tabs.map((tab) => (
        <RailButton
          key={tab.id}
          icon={tab.icon}
          tooltip={tooltip(tab.title)}
          isOn={tabShown === tab.id}
          onClick={() => onTab(tab.id)}
        />
      ))}
    </div>
  );
}
