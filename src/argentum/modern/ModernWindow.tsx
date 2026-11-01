/**
 * The Modern window: the whole app, at every window size. Ours.
 *
 * One bar per edge. The top bar is also the title bar; the left column holds the
 * folders and the history; the centre shows the folder's grid or the open photo
 * and nothing else; the filmstrip runs under the centre in both views; the right
 * panel holds every tool, under one histogram. Opening a photo swaps only the
 * centre: the columns, the filmstrip and the top bar stay where they are, and
 * Settings opens in the centre too.
 *
 * Every part inside is today's: their folder tree, library grid, editor,
 * filmstrip and panels, arranged here and drawn in the Modern look
 * (modern.css). App.tsx passes its handlers through and renders only this;
 * Classic's title bar, side panels and views are gone.
 */

import {
  type ComponentProps,
  type MouseEvent,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
  type RefObject,
  useEffect,
  useState,
} from 'react';
import { useShallow } from 'zustand/react/shallow';
import clsx from 'clsx';
import Editor from '../../components/panel/Editor';
import LibraryView from '../../components/views/LibraryView';
import {
  type ImageFile,
  type LibraryViewMode,
  Panel,
  type ThumbnailAspectRatio,
} from '../../components/ui/AppProperties';
import { useEditorStore } from '../../store/useEditorStore';
import { useLibraryStore } from '../../store/useLibraryStore';
import { useSettingsStore } from '../../store/useSettingsStore';
import { DEFAULT_BOTTOM_PANEL_HEIGHT, DEFAULT_PANEL_WIDTH, useUIStore } from '../../store/useUIStore';
import { useEditorLayout } from '../layout/editorLayout';
import type { TransformHandle } from '../layout/Navigator';
import FilmstripArea from './FilmstripArea';
import { useModernFolders } from './folders';
import LeftColumn from './LeftColumn';
import ModernSettings, { type SettingsProps } from './ModernSettings';
import { DockedNavigator, FloatingNavigator } from './NavigatorDock';
import RightPanel from './RightPanel';
import TopBar from './TopBar';

type Resize = (stateKey: string, startSize: number) => (e: ReactPointerEvent<HTMLDivElement>) => void;

/**
 * The columns' widths: the window's own (left 268, right 330) while the shared
 * width is still the app's untouched default, else the one dragged to, within
 * the window's limits.
 */
const LEFT = { start: 268, min: 224, max: 400 };
const RIGHT = { start: 330, min: 300, max: 480 };
const widthOf = (stored: number, c: typeof LEFT) =>
  stored === DEFAULT_PANEL_WIDTH ? c.start : Math.min(c.max, Math.max(c.min, stored));

/** The grid's props, with the view tools the top bar sets. */
type LibraryProps = ComponentProps<typeof LibraryView> & {
  setThumbnailAspectRatio(ratio: ThumbnailAspectRatio): void;
  setLibraryViewMode(mode: LibraryViewMode): void;
};

/** Under this much room for the centre, the left column folds by itself; the tools never do. */
const CENTRE_MIN_PX = 480;

export interface ModernWindowProps {
  /** The GPU draws the photo: the window is see-through where the photo is. */
  gpu: boolean;
  renderAppPanel(panel: Panel): ReactNode;
  createResizeHandler: Resize;
  createResizeResetHandler(stateKey: string): () => void;
  /** Fill the folder column (folders.ts startModernLibrary). */
  onStart(): void;
  onSelectFolder(path: string): void;
  onNavBack(): void;
  onNavForward(): void;
  // The photo.
  transformWrapperRef: RefObject<TransformHandle | null>;
  onBackToLibrary(): void;
  onEditorContextMenu(event: MouseEvent): void;
  onOpenInEditor(path: string, event?: MouseEvent): void;
  onOpenPhoto(path: string): void;
  onZoom(zoom: number, fitToWindow?: boolean): void;
  // The grid and the filmstrip.
  libraryProps: LibraryProps;
  sortedImageList: ImageFile[];
  thumbnailAspectRatio: ThumbnailAspectRatio;
  onSelectInGrid(path: string, event: MouseEvent): void;
  onStepPhoto(step: 1 | -1): void;
  onClearSelection(): void;
  onThumbnailContextMenu(event: MouseEvent, path: string): void;
  onEmptyAreaContextMenu(event: MouseEvent): void;
  onRequestThumbnails(paths: string[]): void;
  settingsProps: SettingsProps;
  /** Today's bar for a photo being edited for another app (Done), above the photo. */
  externalEditBar?: ReactNode;
}

export default function ModernWindow(props: ModernWindowProps) {
  const { uiVisibility, leftPanelWidth, rightPanelWidth, bottomPanelHeight, activeView, isSettingsOpen, isFullScreen } =
    useUIStore(
      useShallow((s) => ({
        uiVisibility: s.uiVisibility,
        leftPanelWidth: s.leftPanelWidth,
        rightPanelWidth: s.rightPanelWidth,
        bottomPanelHeight: s.bottomPanelHeight,
        activeView: s.activeView,
        isSettingsOpen: s.isSettingsOpen,
        isFullScreen: s.isFullScreen,
      })),
    );
  const selectedImage = useEditorStore((s) => s.selectedImage);
  const noFolders = useLibraryStore((s) => s.rootPaths.length === 0);
  const unavailable = useModernFolders((s) => s.unavailable);
  // The folder last open is in the settings, so the start waits for them.
  const settingsRead = useSettingsStore((s) => s.appSettings !== null);
  const editing = activeView === 'editor' && !!selectedImage;
  const navigatorFloats = useEditorLayout((s) => s.panels.navigatorFloating);
  const navigator = { transformWrapperRef: props.transformWrapperRef, onZoomChange: props.onZoom };
  const sides = !isFullScreen;
  const left = widthOf(leftPanelWidth, LEFT);
  const right = widthOf(rightPanelWidth, RIGHT);
  const windowWidth = useWindowWidth();
  const roomForLeft = windowWidth - left - (uiVisibility.rightPanel ? right : 0) >= CENTRE_MIN_PX;

  // No start screen: with no folders yet (the app has just opened, or Esc left
  // the folder), the column is filled at once.
  const { onStart } = props;
  useEffect(() => {
    if (noFolders && !unavailable && settingsRead) onStart();
  }, [noFolders, unavailable, settingsRead, onStart]);

  return (
    <div className={clsx('agm-window', props.gpu && editing && 'agm-gpu')}>
      {sides && (
        <TopBar
          onBack={props.onNavBack}
          onForward={props.onNavForward}
          onOpenPath={props.onSelectFolder}
          onOpenPhoto={props.onOpenPhoto}
          onBackToLibrary={props.onBackToLibrary}
          onZoom={props.onZoom}
          thumbnailSize={props.libraryProps.thumbnailSize}
          setThumbnailSize={props.libraryProps.setThumbnailSize}
          thumbnailAspectRatio={props.libraryProps.thumbnailAspectRatio}
          setThumbnailAspectRatio={props.libraryProps.setThumbnailAspectRatio}
          libraryViewMode={props.libraryProps.libraryViewMode}
          setLibraryViewMode={props.libraryProps.setLibraryViewMode}
        />
      )}
      <div className="agm-body">
        {sides && uiVisibility.leftPanel && roomForLeft && (
          <LeftColumn
            width={left}
            editing={editing}
            navigator={editing && !navigatorFloats && <DockedNavigator {...navigator} />}
            renderPanel={props.renderAppPanel}
            onResize={props.createResizeHandler('left', left)}
            onResetWidth={props.createResizeResetHandler('left')}
          />
        )}
        <div className="agm-centre">
          {props.externalEditBar}
          <div className={clsx('agm-view', !editing && 'agm-grid-view')}>
            <div className={clsx('flex-1 flex flex-col min-h-0 relative', !editing && 'hidden')}>
              {selectedImage && (
                <>
                  <Editor onContextMenu={props.onEditorContextMenu} transformWrapperRef={props.transformWrapperRef} />
                  {navigatorFloats && <FloatingNavigator {...navigator} />}
                  {/* Never shown: bench/replay.js leaves the photo by clicking this, as it did their
                      toolbar's back button. People press Esc, which runs the same handler. */}
                  <button className="hidden" data-bench-id="back-to-library" onClick={props.onBackToLibrary} />
                </>
              )}
            </div>
            <div className={clsx('flex-1 flex flex-col min-h-0', editing && 'hidden')}>
              <LibraryView {...props.libraryProps} />
            </div>
            {isSettingsOpen && (
              <div className="absolute inset-0 z-30 flex">
                <ModernSettings {...props.settingsProps} />
              </div>
            )}
          </div>
          {sides && uiVisibility.filmstrip && !isSettingsOpen && (
            <FilmstripArea
              imageList={props.sortedImageList}
              thumbnailAspectRatio={props.thumbnailAspectRatio}
              onOpenInEditor={props.onOpenInEditor}
              onSelectInGrid={props.onSelectInGrid}
              onStepPhoto={props.onStepPhoto}
              onClearSelection={props.onClearSelection}
              onContextMenu={props.onThumbnailContextMenu}
              onEmptyAreaContextMenu={props.onEmptyAreaContextMenu}
              onRequestThumbnails={props.onRequestThumbnails}
              onResize={props.createResizeHandler(
                'bottom',
                bottomPanelHeight === DEFAULT_BOTTOM_PANEL_HEIGHT ? 104 : bottomPanelHeight,
              )}
              onResetHeight={props.createResizeResetHandler('bottom')}
            />
          )}
        </div>
        {sides && uiVisibility.rightPanel && (
          <RightPanel
            width={right}
            renderPanel={props.renderAppPanel}
            onOpenPhoto={props.onOpenPhoto}
            onResize={props.createResizeHandler('right', right)}
            onResetWidth={props.createResizeResetHandler('right')}
          />
        )}
      </div>
    </div>
  );
}

function useWindowWidth(): number {
  const [width, setWidth] = useState(() => window.innerWidth);
  useEffect(() => {
    const onResize = () => setWidth(window.innerWidth);
    window.addEventListener('resize', onResize);
    return () => window.removeEventListener('resize', onResize);
  }, []);
  return width;
}
