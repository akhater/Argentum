// What the window has inside the app, in the top bar: Open, the photo the app
// was started with, a quiet line saying edits are not saved yet, and how long
// the last change took to reach pixels.

import { FolderOpen } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';

import { text } from '../i18n';
import { useShellStore } from '../ShellContext';
import type { ShellStore } from '../store';
import type { AppConnection, Library } from './client';
import './app.css';

const nameOf = (path: string) => path.split(/[\\/]/).pop();

/** Open a photo from disk, its folder becoming the filmstrip. */
export async function showFolderOf(store: ShellStore, library: Library, path: string): Promise<void> {
  let photos: string[] = [];
  try {
    photos = await library.folder(path);
  } catch (e) {
    store.getState().fail(e);
  }
  // The folder's own spelling of the path, so the filmstrip marks it.
  const same = photos.find((p) => nameOf(p) === nameOf(path)) ?? path;
  store.setState({ photos: photos.includes(same) ? photos : [same, ...photos] });
  await store.getState().openPhoto(same);
}

export function AppBar({ app }: { app: AppConnection }) {
  const store = useShellStore();
  const [tickMs, setTickMs] = useState<number | null>(null);
  const startedWith = useRef<string | null>(null);

  useEffect(() => {
    if (app.first && startedWith.current !== app.first) {
      startedWith.current = app.first;
      void showFolderOf(store, app.library, app.first);
    }
    return app.onTickMs(setTickMs);
  }, [store, app]);

  const pick = async () => {
    try {
      const path = await app.library.pick(store.getState().path);
      if (path) await showFolderOf(store, app.library, path);
    } catch (e) {
      store.getState().fail(e);
    }
  };

  return (
    <>
      <button type="button" className="ags-app-open" onClick={() => void pick()}>
        <FolderOpen size={14} />
        <span>{text('shell.open')}</span>
      </button>
      <span className="ags-app-note">
        {text('shell.notSaved')}
        {tickMs !== null && `  ·  ${text('shell.tickMs', { ms: Math.round(tickMs) })}`}
      </span>
    </>
  );
}
