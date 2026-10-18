// The strip along the bottom, 128 px as RAWmakase's (src/app/library/filmstrip.rs,
// MIT), with a status line above it. The open photo shows its picture; the
// others show their names, since the contract has no thumbnails yet.

import { text } from '../i18n';
import { fileName } from '../paths';
import { useShell, useShellStore } from '../ShellContext';
import { PhotoPicture } from './PhotoPicture';

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

export function Filmstrip() {
  const store = useShellStore();
  const photos = useShell((s) => s.photos);
  const current = useShell((s) => s.path);
  const frame = useShell((s) => s.fitFrame);
  return (
    <div className="ags-filmstrip">
      {photos.map((path) => (
        <button
          key={path}
          type="button"
          className={path === current ? 'ags-film is-current' : 'ags-film'}
          title={path}
          onClick={() => path !== current && void store.getState().openPhoto(path)}
        >
          {path === current && <PhotoPicture frame={frame} className="ags-film-picture" />}
          <span className="ags-film-name">{fileName(path)}</span>
        </button>
      ))}
    </div>
  );
}
