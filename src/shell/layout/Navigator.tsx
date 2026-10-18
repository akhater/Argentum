// The whole photo, from its last fitted frame, with the part on screen marked
// while zoomed in. A click there moves the view to it.

import type { PointerEvent } from 'react';

import { text } from '../i18n';
import { useShell, useShellStore } from '../ShellContext';
import { deviceScale, fitRect, visiblePart, type RectPx } from '../view';
import { currentView } from '../viewSlice';
import { PhotoPicture } from './PhotoPicture';

export function Navigator() {
  const store = useShellStore();
  const frame = useShell((s) => s.fitFrame);
  const photo = useShell((s) => s.photo);
  const zoom = useShell((s) => s.zoom);
  const center = useShell((s) => s.center);
  const viewSize_px = useShell((s) => s.viewSize_px);

  const marker = (ctx: CanvasRenderingContext2D, drawn: RectPx) => {
    const view = currentView({ photo, viewSize_px, zoom, center });
    if (zoom === 'fit' || !view || !photo) return;
    const part = visiblePart(view, photo);
    ctx.strokeStyle = 'rgba(255,255,255,0.9)';
    ctx.lineWidth = Math.max(1, deviceScale());
    ctx.strokeRect(
      drawn.x_px + part.x * drawn.width_px,
      drawn.y_px + part.y * drawn.height_px,
      part.width * drawn.width_px,
      part.height * drawn.height_px,
    );
  };

  const goTo = (e: PointerEvent<HTMLDivElement>) => {
    const s = store.getState();
    if (!s.photo) return;
    const box = e.currentTarget.getBoundingClientRect();
    const drawn = fitRect(
      { width_px: s.photo.width, height_px: s.photo.height },
      { width_px: box.width, height_px: box.height },
    );
    const point = {
      x: (e.clientX - box.left - drawn.x_px) / drawn.width_px,
      y: (e.clientY - box.top - drawn.y_px) / drawn.height_px,
    };
    if (s.zoom === 'fit') s.setZoom(1, point);
    else s.setCenter(point);
  };

  return (
    <div className="ags-panel-block">
      <div className="ags-block-head">
        <span>{text('shell.navigator')}</span>
        <span className="ags-zoom-buttons">
          <button
            type="button"
            className={zoom === 'fit' ? 'is-on' : ''}
            onClick={() => store.getState().setZoom('fit')}
          >
            {text('shell.fit')}
          </button>
          <button type="button" className={zoom === 1 ? 'is-on' : ''} onClick={() => store.getState().setZoom(1)}>
            {text('shell.oneToOne')}
          </button>
        </span>
      </div>
      <div className="ags-navigator" onPointerDown={goTo}>
        <PhotoPicture frame={frame} className="ags-navigator-canvas" overlay={marker} />
      </div>
    </div>
  );
}
