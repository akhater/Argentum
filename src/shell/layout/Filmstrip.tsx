// The strip along the bottom, 128 px as RAWmakase's (src/app/library/filmstrip.rs,
// MIT), with a status line above it: the folder browsed, drawn only as far as
// it shows (react-window), each photo's thumbnail asked for as it comes into
// view. The open photo shows its live picture, edits and all.

import { Grid, type CellComponentProps } from 'react-window';

import { Thumb } from '../browse/Thumb';
import { text } from '../i18n';
import { fileName, samePath } from '../paths';
import { useShell, useShellStore } from '../ShellContext';
import { PhotoPicture } from './PhotoPicture';

/** A frame's width and the gap after it; its height is the strip's. */
const FILM_PX = 140;
const FILM_GAP_PX = 6;
const FILM_HEIGHT_PX = 112;
/** Frames past either end of the strip whose thumbnails are asked for too. */
const FRAMES_AHEAD = 6;

export function StatusLine({ fake }: { fake: boolean }) {
  const path = useShell((s) => s.path);
  const photo = useShell((s) => s.photo);
  const error = useShell((s) => s.error);
  return (
    <div className="ags-status">
      <span>{path ? `${fileName(path)}  ·  ${photo?.width} × ${photo?.height}` : text('shell.noPhoto')}</span>
      <span className="ags-toolbar-spacer" />
      {error ? (
        <span className="ags-error" role="alert">
          {error.message}
        </span>
      ) : (
        fake && <span className="ags-dim">{text('shell.fakeEngineHint')}</span>
      )}
    </div>
  );
}

interface FilmProps {
  photos: readonly string[];
  current: string | null;
}

function Film({ columnIndex, style, photos, current }: CellComponentProps<FilmProps>) {
  const store = useShellStore();
  const path = photos[columnIndex];
  const isCurrent = current !== null && samePath(path, current);
  const frame = useShell((s) => (isCurrent ? s.fitFrame : null));
  const photo = useShell((s) => (isCurrent ? s.photo : null));
  return (
    <div style={style}>
      <button
        type="button"
        className={isCurrent ? 'ags-film is-current' : 'ags-film'}
        title={path}
        onClick={() => !isCurrent && void store.getState().edit(path)}
      >
        {isCurrent && frame ? (
          <PhotoPicture frame={frame} photo={photo} className="ags-film-picture" />
        ) : (
          <Thumb path={path} />
        )}
        <span className="ags-film-name">{fileName(path)}</span>
      </button>
    </div>
  );
}

export function Filmstrip() {
  const store = useShellStore();
  const photos = useShell((s) => s.photos);
  const current = useShell((s) => s.path);
  return (
    <div className="ags-filmstrip">
      <Grid
        cellComponent={Film}
        cellProps={{ photos, current }}
        columnCount={photos.length}
        columnWidth={FILM_PX + FILM_GAP_PX}
        rowCount={1}
        rowHeight={FILM_HEIGHT_PX}
        overscanCount={4}
        defaultWidth={1200}
        defaultHeight={FILM_HEIGHT_PX}
        style={{ overflowY: 'hidden' }}
        onWheel={(e) => {
          // A mouse wheel scrolls the strip along.
          if (e.deltaY !== 0 && Math.abs(e.deltaX) < Math.abs(e.deltaY)) e.currentTarget.scrollLeft += e.deltaY;
        }}
        onCellsRendered={(visible) => {
          const first = Math.max(0, visible.columnStartIndex - FRAMES_AHEAD);
          const shown = photos.slice(visible.columnStartIndex, visible.columnStopIndex + 1);
          const around = [
            ...photos.slice(visible.columnStopIndex + 1, visible.columnStopIndex + 1 + FRAMES_AHEAD),
            ...photos.slice(first, visible.columnStartIndex),
          ];
          store.getState().wantThumbs([...shown, ...around]);
        }}
      />
    </div>
  );
}
