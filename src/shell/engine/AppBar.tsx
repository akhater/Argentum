// What the window has inside the app, in the top bar: Open, the photo the app
// was started with, a quiet line saying edits are not saved yet, and how long
// the last change took to reach pixels.

import { FolderOpen } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';

import { text } from '../i18n';
import { useShellStore } from '../ShellContext';
import type { AppConnection } from './client';
import './app.css';

export function AppBar({ app }: { app: AppConnection }) {
  const store = useShellStore();
  const [tickMs, setTickMs] = useState<number | null>(null);
  const startedWith = useRef<string | null>(null);

  useEffect(() => {
    if (app.first && startedWith.current !== app.first) {
      startedWith.current = app.first;
      void store.getState().openFolder(app.first);
    }
    return app.onTickMs(setTickMs);
  }, [store, app]);

  const pick = async () => {
    try {
      const path = await app.library.pick(store.getState().path);
      if (path) await store.getState().openFolder(path);
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
