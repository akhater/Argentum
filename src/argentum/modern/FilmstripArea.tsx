/**
 * The Modern window's filmstrip, under the centre in both views. Ours.
 *
 * Its header carries what used to need a bar of its own: the folder, which
 * photo of how many, its name, how many are selected, and on the right the
 * status line (render time while a photo is open, thumbnail progress in the
 * grid). The strip is today's Filmstrip. A click opens the photo while one is
 * open, and selects it in the grid, as the grid's own click does. The arrows
 * either side of it step as Left and Right do, in a column of their own, so
 * they never cover a thumbnail.
 */

import { type MouseEvent, type PointerEvent as ReactPointerEvent } from 'react';
import { useShallow } from 'zustand/react/shallow';
import { ChevronLeft, ChevronRight } from 'lucide-react';
import Filmstrip from '../../components/panel/Filmstrip';
import Resizer from '../../components/ui/Resizer';
import { type ImageFile, Orientation, type ThumbnailAspectRatio } from '../../components/ui/AppProperties';
import { useEditorStore } from '../../store/useEditorStore';
import { useLibraryStore } from '../../store/useLibraryStore';
import { useProcessStore } from '../../store/useProcessStore';
import { DEFAULT_BOTTOM_PANEL_HEIGHT, useUIStore } from '../../store/useUIStore';
import RenderStatus from '../RenderStatus';
import { useAgTranslation } from '../locales';
import { folderName } from './folders';
import { fileNameOf } from '../paths';

/** The header's height; the strip under it is the resizable part. */
const HEADER_PX = 24;
/** The strip's own height while the shared one is still the app's untouched default. */
const STRIP_PX = 104;
const PREVIOUS = { step: -1, label: 'mwPrevPhoto', Icon: ChevronLeft } as const;
const NEXT = { step: 1, label: 'mwNextPhoto', Icon: ChevronRight } as const;

interface FilmstripAreaProps {
  imageList: ImageFile[];
  thumbnailAspectRatio: ThumbnailAspectRatio;
  onOpenInEditor(path: string, event?: MouseEvent): void;
  onSelectInGrid(path: string, event: MouseEvent): void;
  onStepPhoto(step: 1 | -1): void;
  onClearSelection(): void;
  onContextMenu(event: MouseEvent, path: string): void;
  onEmptyAreaContextMenu(event: MouseEvent): void;
  onRequestThumbnails(paths: string[]): void;
  onResize(e: ReactPointerEvent<HTMLDivElement>): void;
  onResetHeight(): void;
}

export default function FilmstripArea(props: FilmstripAreaProps) {
  const t = useAgTranslation();
  const { activeView, stored } = useUIStore(
    useShallow((s) => ({ activeView: s.activeView, stored: s.bottomPanelHeight })),
  );
  const height = stored === DEFAULT_BOTTOM_PANEL_HEIGHT ? STRIP_PX : stored;
  const selectedImage = useEditorStore((s) => s.selectedImage);
  const { currentFolderPath, libraryActivePath, multiSelectedPaths, imageRatings, isViewLoading } = useLibraryStore(
    useShallow((s) => ({
      currentFolderPath: s.currentFolderPath,
      libraryActivePath: s.libraryActivePath,
      multiSelectedPaths: s.multiSelectedPaths,
      imageRatings: s.imageRatings,
      isViewLoading: s.isViewLoading,
    })),
  );
  const progress = useProcessStore((s) => s.thumbnailProgress);
  const editing = activeView === 'editor' && !!selectedImage;
  const current = editing ? selectedImage?.path : libraryActivePath;
  const index = current ? props.imageList.findIndex((f) => f.path === current) : -1;
  const total = props.imageList.length;

  // Its column stays with no photo current, so the strip keeps its width. A press takes no focus,
  // so Space and Enter stay the window's shortcuts after a click.
  const arrow = ({ step, label, Icon }: typeof PREVIOUS | typeof NEXT) => (
    <div className="agm-strip-side">
      {index >= 0 && (
        <button
          className="agm-btn"
          aria-label={t(label)}
          data-tooltip={t(label)}
          disabled={total < 2}
          onMouseDown={(event) => event.preventDefault()}
          onClick={() => props.onStepPhoto(step)}
        >
          <Icon size={18} />
        </button>
      )}
    </div>
  );

  return (
    <div className="agm-strip" style={{ height: height + HEADER_PX }}>
      <Resizer direction={Orientation.Horizontal} onMouseDown={props.onResize} onDoubleClick={props.onResetHeight} />
      <div className="agm-strip-head">
        {currentFolderPath ? (
          <>
            <span className="agm-strip-src">{folderName(currentFolderPath)}</span>
            {index >= 0 && (
              <span>
                {t('mwOf')
                  .replace('{n}', String(index + 1))
                  .replace('{total}', String(total))}
              </span>
            )}
            {current && <span>{fileNameOf(current)}</span>}
            {multiSelectedPaths.length > 1 && (
              <span>{t('mwSelected').replace('{count}', String(multiSelectedPaths.length))}</span>
            )}
          </>
        ) : (
          <span>{t('mwNoFolder')}</span>
        )}
        <span className="agm-strip-right">
          {editing ? (
            <RenderStatus />
          ) : (
            currentFolderPath && (
              <>
                <span>{t('mwThumbsEdits')}</span>
                {progress.total > 0 && (
                  <span>
                    {t('mwThumbsReady')
                      .replace('{n}', String(progress.current))
                      .replace('{total}', String(progress.total))}
                  </span>
                )}
              </>
            )
          )}
        </span>
      </div>
      <div className="agm-strip-body">
        {arrow(PREVIOUS)}
        <div className="agm-strip-film">
          <Filmstrip
            imageList={props.imageList}
            imageRatings={imageRatings}
            isLoading={isViewLoading}
            multiSelectedPaths={multiSelectedPaths}
            onClearSelection={props.onClearSelection}
            onContextMenu={props.onContextMenu}
            onEmptyAreaContextMenu={props.onEmptyAreaContextMenu}
            onImageSelect={(path, event) =>
              editing ? props.onOpenInEditor(path, event) : props.onSelectInGrid(path, event)
            }
            onRequestThumbnails={props.onRequestThumbnails}
            selectedImage={editing ? (selectedImage ?? undefined) : undefined}
            thumbnailAspectRatio={props.thumbnailAspectRatio}
            totalImages={total}
          />
        </div>
        {arrow(NEXT)}
      </div>
    </div>
  );
}
