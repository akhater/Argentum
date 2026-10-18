// The whole photo, from its last fitted frame, with the part on screen marked
// while zoomed in. A click there moves the view to it.

import type { PointerEvent } from 'react';

import { text } from '../i18n';
import { useShell, useShellStore } from '../ShellContext';
import { viewOf } from '../store';
import { PhotoPicture, type PhotoRect } from './PhotoPicture';

export function Navigator() {
  const store = useShellStore();
  const frame = useShell((s) => s.fitFrame);
  const photo = useShell((s) => s.photo);
  const zoom = useShell((s) => s.zoom);
  const center = useShell((s) => s.center);
  const viewSize = useShell((s) => s.viewSize);

  const marker = (ctx: CanvasRenderingContext2D, r: PhotoRect) => {
    const view = viewOf({ photo, viewSize, zoom, center });
    if (zoom === 'fit' || !view || !photo) return;
    const w = view.width / (view.zoom * photo.width);
    const h = view.height / (view.zoom * photo.height);
    const x = Math.max(0, view.centerX - w / 2);
    const y = Math.max(0, view.centerY - h / 2);
    ctx.strokeStyle = 'rgba(255,255,255,0.9)';
    ctx.lineWidth = Math.max(1, window.devicePixelRatio || 1);
    ctx.strokeRect(r.x + x * r.width, r.y + y * r.height, Math.min(1, w) * r.width, Math.min(1, h) * r.height);
  };

  const goTo = (e: PointerEvent<HTMLDivElement>) => {
    const s = store.getState();
    if (!s.photo) return;
    const box = e.currentTarget.getBoundingClientRect();
    const aspect = s.photo.width / s.photo.height;
    const w = Math.min(box.width, box.height * aspect);
    const h = w / aspect;
    const x = (e.clientX - box.left - (box.width - w) / 2) / w;
    const y = (e.clientY - box.top - (box.height - h) / 2) / h;
    if (s.zoom === 'fit') s.setZoom(1, { x, y });
    else s.setCenter(x, y);
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
        <PhotoPicture frame={frame} photo={photo} className="ags-navigator-canvas" overlay={marker} />
      </div>
    </div>
  );
}
