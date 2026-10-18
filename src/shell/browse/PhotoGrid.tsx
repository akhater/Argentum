// The folder's photos as a grid of thumbnails, drawn only as far as the
// screen shows them (react-window), asking for the thumbnails on screen
// first, then a few rows ahead. Click picks a photo, double-click or Enter
// opens it in Edit, the arrow keys move.

import { Pencil } from 'lucide-react';
import { useEffect, useRef, useState, type KeyboardEvent, type RefObject } from 'react';
import { Grid, useGridRef, type CellComponentProps } from 'react-window';

import { text } from '../i18n';
import { samePath } from '../paths';
import { useShell, useShellStore } from '../ShellContext';
import { clamp } from '../view';
import { THUMB_PX, type Photo } from './browse.generated';
import { Thumb } from './Thumb';

/** A cell's picture: a thumbnail's long edge at 2x. */
export const CELL_PX = THUMB_PX / 2;
/** Room for the name under it, and the gap around it. */
const LABEL_PX = 22;
const GAP_PX = 10;
/** Rows past the screen whose thumbnails are asked for, after the screen's. */
const ROWS_AHEAD = 2;

interface CellProps {
  photos: readonly Photo[];
  columns: number;
  selected: string | null;
}

function Cell({ columnIndex, rowIndex, style, photos, columns, selected }: CellComponentProps<CellProps>) {
  const store = useShellStore();
  const photo = photos[rowIndex * columns + columnIndex];
  if (!photo) return null;
  const picked = selected !== null && samePath(selected, photo.path);
  return (
    <div style={style} className="agb-cell-slot">
      <button
        type="button"
        className={picked ? 'agb-cell is-selected' : 'agb-cell'}
        title={photo.path}
        onClick={() => store.getState().select(photo.path)}
        onDoubleClick={() => void store.getState().edit(photo.path)}
      >
        <span className="agb-cell-picture">
          <Thumb path={photo.path} />
          {photo.edited && (
            <span className="agb-edited" title={text('browse.edited')}>
              <Pencil size={11} />
            </span>
          )}
        </span>
        <span className="agb-cell-name">{photo.name}</span>
      </button>
    </div>
  );
}

/** The element's width and height, as it is laid out. */
function useSize(): [RefObject<HTMLDivElement | null>, { width: number; height: number }] {
  const ref = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState({ width: 0, height: 0 });
  useEffect(() => {
    const el = ref.current;
    if (!el || typeof ResizeObserver === 'undefined') return;
    const seen = new ResizeObserver(([entry]) =>
      setSize({ width: entry.contentRect.width, height: entry.contentRect.height }),
    );
    seen.observe(el);
    return () => seen.disconnect();
  }, []);
  return [ref, size];
}

export function PhotoGrid() {
  const store = useShellStore();
  const photos = useShell((s) => s.folderPhotos);
  const selected = useShell((s) => s.selected);
  const [box, size] = useSize();
  const grid = useGridRef(null);
  const columns = Math.max(1, Math.floor(size.width / (CELL_PX + GAP_PX)));
  const rows = Math.ceil(photos.length / columns);
  // The rows on screen, kept to ask again when the photos or columns change.
  const onScreen = useRef({ first: 0, last: 0 });

  const want = () => {
    const { first, last } = onScreen.current;
    const rowPaths = (r: number) => photos.slice(r * columns, (r + 1) * columns).map((p) => p.path);
    const order: number[] = [];
    for (let r = first; r <= last; r++) order.push(r);
    for (let r = last + 1; r <= last + ROWS_AHEAD; r++) order.push(r);
    if (first > 0) order.push(first - 1);
    store.getState().wantThumbs(order.filter((r) => r < rows).flatMap(rowPaths));
  };
  useEffect(want, [photos, columns]);

  const move = (e: KeyboardEvent) => {
    const step = { ArrowLeft: -1, ArrowRight: 1, ArrowUp: -columns, ArrowDown: columns }[e.key];
    const at = photos.findIndex((p) => selected !== null && samePath(p.path, selected));
    if (e.key === 'Enter' && at >= 0) {
      void store.getState().edit(photos[at].path);
    } else if (step !== undefined && photos.length > 0) {
      e.preventDefault();
      const next = clamp(at < 0 ? 0 : at + step, 0, photos.length - 1);
      store.getState().select(photos[next].path);
      grid.current?.scrollToCell({ rowIndex: Math.floor(next / columns), columnIndex: next % columns });
    }
  };

  return (
    <div ref={box} className="agb-grid" tabIndex={0} onKeyDown={move}>
      {photos.length > 0 && size.width > 0 && (
        <Grid
          gridRef={grid}
          cellComponent={Cell}
          cellProps={{ photos, columns, selected }}
          columnCount={columns}
          columnWidth={Math.floor(size.width / columns)}
          rowCount={rows}
          rowHeight={CELL_PX + LABEL_PX + GAP_PX}
          overscanCount={1}
          // The columns already fit: no sideways scrollbar once the vertical one takes its room.
          style={{ width: size.width, height: size.height, overflowX: 'hidden' }}
          onCellsRendered={(visible) => {
            onScreen.current = { first: visible.rowStartIndex, last: visible.rowStopIndex };
            want();
          }}
        />
      )}
    </div>
  );
}
