/**
 * The RAWmakase arrangement of today's editor. Ours.
 *
 * RAWmakase 0.3 (src/app/workspace.rs, side_tabs.rs) lays out its Develop
 * module like Lightroom Classic: the folders down the left; the photo with a
 * toolbar under it; on the right the adjustments with the Navigator and
 * Histogram over them, a tab panel (Presets, History) between them and the
 * photo, and a slim rail at the window's edge that switches both; the
 * filmstrip along the bottom. This builds that out of today's parts and
 * nothing else: today's EditorView (the editor, its toolbar and the filmstrip
 * bar), today's folder tree, today's panels and today's stores. It takes
 * exactly EditorView's props, so App.tsx swaps one for the other and passes
 * the same things to both.
 *
 * Today's files carry only what lets this arrange them, as props: EditorView
 * leaves room beside the photo (besideEditor) and asks for the toolbar under
 * it (toolbarBelow); the Adjustments and Masks panels leave out their own copy
 * of the histogram (showWaveform), since it is here at the top of the column.
 *
 * Widths and folds are today's workspace values, shared with the Classic
 * arrangement and saved with it: the folder column is the left panel's width
 * and fold, the adjustments column the right panel's. The Navigator's fold and
 * the tab panel's tab and width are this arrangement's own (editorLayout.ts).
 */

import { type ComponentProps, type PointerEvent, type ReactNode, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Folder } from 'lucide-react';
import EditorView from '../../components/views/EditorView';
import Controls from '../../components/panel/right/ControlsPanel';
import MasksPanel from '../../components/panel/right/MasksPanel';
import Resizer from '../../components/ui/Resizer';
import { Orientation, Panel } from '../../components/ui/AppProperties';
import { DEFAULT_PANEL_WIDTH, useUIStore } from '../../store/useUIStore';
import { MAX_TAB_WIDTH_PX, MIN_TAB_WIDTH_PX, useEditorLayout } from './editorLayout';
import { SIDE_TABS, type SideTabId, toolShown } from './rail';
import HistogramSection from './HistogramSection';
import HistoryPanel from './HistoryPanel';
import Navigator from './Navigator';
import ToolRail from './ToolRail';

type EditorViewProps = ComponentProps<typeof EditorView>;

/** The rail's width, and a folded column's (side_tabs.rs RAIL_WIDTH). */
const RAIL_WIDTH_PX = 36;
/** The resize handle between columns (their Resizer, w-2). */
const HANDLE_PX = 8;
/** The photo's narrowest width before the tab panel folds by itself (side_tabs.rs PHOTO_MIN). */
const PHOTO_MIN_PX = 400;
/**
 * What the tool panel keeps of a short column. The Navigator and Histogram
 * take the rest and scroll inside it; folding either gives the room back.
 */
const TOOL_MIN_PX = 360;

/** A column of today's: their panel card around one of today's panels. */
function Card({ children }: { children: ReactNode }) {
  return (
    <div className="flex-1 min-h-0 min-w-0 bg-bg-secondary rounded-lg border border-surface overflow-hidden relative">
      <div className="absolute inset-0 overflow-y-auto custom-scrollbar">{children}</div>
    </div>
  );
}

export default function RawmakaseEditor(props: EditorViewProps) {
  const { renderAppPanel, createResizeHandler, createResizeResetHandler, transformWrapperRef, handleZoomChange } =
    props;
  const { t } = useTranslation();
  const isFullScreen = useUIStore((s) => s.isFullScreen);
  // App keeps the editor mounted, hidden, while the library shows, as it
  // always has; the columns around it are today's side panels' to show then.
  const isEditing = useUIStore((s) => s.activeView === 'editor');
  const showsColumns = isEditing && !isFullScreen;
  const folderShown = useUIStore((s) => s.uiVisibility.leftPanel);
  const folderWidth = useUIStore((s) => s.leftPanelWidth);
  const columnShown = useUIStore((s) => s.uiVisibility.rightPanel);
  const columnWidth = useUIStore((s) => s.rightPanelWidth);
  const activePanel = useUIStore((s) => s.activePanel);
  const panelLayout = useUIStore((s) => s.panelLayout);
  const setUI = useUIStore((s) => s.setUI);
  const tab = useEditorLayout((s) => s.panels.tab);
  const tabWidth = useEditorLayout((s) => s.panels.tabWidthPx);
  const changePanels = useEditorLayout((s) => s.changePanels);

  // Tethering is in today's workspace only where it is supported.
  const hasPanel = (panel: Panel) => Object.values(panelLayout).some((panels) => panels.includes(panel));
  const tabs = SIDE_TABS.filter((entry) => entry.id === 'history' || hasPanel(entry.id));
  const tabChosen = tabs.some((entry) => entry.id === tab) ? tab : null;

  // A window too narrow for the tab panel and the photo shows it folded, until
  // a tab is asked for (side_tabs.rs): the choice is kept for a wider window.
  const rowRef = useRef<HTMLDivElement>(null);
  const [rowWidth, setRowWidth] = useState(0);
  const [peek, setPeek] = useState(false);
  useEffect(() => {
    const row = rowRef.current;
    if (!row) return;
    const observer = new ResizeObserver(() => setRowWidth(row.clientWidth));
    observer.observe(row);
    return () => observer.disconnect();
  }, []);
  const photoWidth =
    rowWidth -
    (folderShown ? folderWidth : RAIL_WIDTH_PX + HANDLE_PX) -
    (columnShown ? columnWidth + HANDLE_PX : 0) -
    (RAIL_WIDTH_PX + HANDLE_PX) -
    (tabWidth + HANDLE_PX);
  const hasRoom = rowWidth === 0 || photoWidth >= PHOTO_MIN_PX;
  const tabShown = tabChosen && (hasRoom || peek) ? tabChosen : null;

  const showFolders = () =>
    setUI((s) => ({
      uiVisibility: { ...s.uiVisibility, leftPanel: true },
      leftPanelWidth: Math.max(s.leftPanelWidth, DEFAULT_PANEL_WIDTH),
    }));

  // Today's shortcuts open Presets, Info, Export and the folders through
  // setPanel; here those are tabs and the folder column, so a change of
  // activePanel to one of them opens it. Only a change: the panel last open in
  // the Classic arrangement does not override the tab kept for this one.
  const lastPanel = useRef(activePanel);
  useEffect(() => {
    if (lastPanel.current === activePanel) return;
    lastPanel.current = activePanel;
    if (!isEditing) return;
    if (activePanel === Panel.FolderTree) {
      showFolders();
    } else {
      const asked = tabs.find((entry) => entry.id === activePanel);
      if (asked) {
        changePanels({ tab: asked.id });
        setPeek(!hasRoom);
      }
    }
    // Follows activePanel alone; the rest is read as it stands when it changes.
  }, [activePanel]);

  const pressTab = (id: SideTabId) => {
    if (tabShown !== id) {
      changePanels({ tab: id });
      setPeek(!hasRoom);
    } else if (peek && !hasRoom) {
      setPeek(false);
    } else {
      changePanels({ tab: null });
    }
  };

  const dragTab = (e: PointerEvent<HTMLDivElement>) => {
    if (e.pointerType === 'mouse' && e.button !== 0) return;
    e.preventDefault();
    const startX = e.clientX;
    const start = tabWidth;
    const move = (m: globalThis.PointerEvent) => {
      const width = Math.round(start - (m.clientX - startX));
      changePanels({ tabWidthPx: Math.max(MIN_TAB_WIDTH_PX, Math.min(MAX_TAB_WIDTH_PX, width)) });
    };
    const stop = () => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', stop);
      window.removeEventListener('pointercancel', stop);
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', stop);
    window.addEventListener('pointercancel', stop);
  };

  const tool = toolShown(activePanel);
  // The two panels that carry their own histogram leave it out here.
  const toolPanel =
    tool === Panel.Adjustments ? (
      <Controls showWaveform={false} />
    ) : tool === Panel.Masks ? (
      <MasksPanel showWaveform={false} />
    ) : (
      renderAppPanel(tool)
    );

  const besideEditor = !showsColumns ? null : (
    <div className="flex flex-row shrink-0 h-full min-h-0">
      {tabShown && (
        <>
          <Resizer direction={Orientation.Vertical} onMouseDown={dragTab} />
          <div className="flex shrink-0 h-full" style={{ width: tabWidth }}>
            <Card>{tabShown === 'history' ? <HistoryPanel /> : renderAppPanel(tabShown)}</Card>
          </div>
        </>
      )}
      {columnShown && (
        <>
          <Resizer
            direction={Orientation.Vertical}
            onMouseDown={createResizeHandler('right', columnWidth)}
            onDoubleClick={createResizeResetHandler('right')}
          />
          <div className="flex flex-col shrink-0 h-full min-h-0 gap-2" style={{ width: columnWidth }}>
            <div className="shrink min-h-0 overflow-y-auto custom-scrollbar bg-bg-secondary rounded-lg border border-surface p-2 flex flex-col gap-2">
              <Navigator transformWrapperRef={transformWrapperRef} onZoomChange={handleZoomChange} />
              <HistogramSection />
            </div>
            <div className="flex-1 flex" style={{ minHeight: TOOL_MIN_PX }}>
              <Card>{toolPanel}</Card>
            </div>
          </div>
        </>
      )}
      <div className="w-2 shrink-0" />
      <ToolRail tabs={tabs} tabShown={tabShown} onTab={pressTab} />
    </div>
  );

  return (
    <div ref={rowRef} className="flex flex-row grow h-full min-h-0 min-w-0">
      {showsColumns &&
        (folderShown ? (
          <div className="flex shrink-0 h-full" style={{ width: folderWidth }}>
            <Card>{renderAppPanel(Panel.FolderTree)}</Card>
            <Resizer
              direction={Orientation.Vertical}
              onMouseDown={createResizeHandler('left', folderWidth)}
              onDoubleClick={createResizeResetHandler('left')}
            />
          </div>
        ) : (
          <div className="flex shrink-0 h-full">
            <div className="w-9 h-full bg-bg-secondary rounded-lg border border-surface flex flex-col items-center py-3">
              <button
                className="w-7 h-7 rounded-md flex items-center justify-center text-text-secondary hover:bg-surface hover:text-text-primary transition-colors"
                onClick={showFolders}
                data-tooltip={t('library.folders.sourcesTitle')}
              >
                <Folder size={16} />
              </button>
            </div>
            <div className="w-2 shrink-0" />
          </div>
        ))}
      <div className="flex-1 flex flex-col min-w-0 min-h-0">
        <EditorView {...props} besideEditor={besideEditor} toolbarBelow />
      </div>
    </div>
  );
}
