// The new editor interface. Layout after RAWmakase 0.3.3's Develop
// (src/app/workspace.rs and side_tabs.rs @ 7c3c81ab, MIT), itself Lightroom Classic's:
// from the window's right edge the rail, the tool panels under the histogram
// and navigator, the Presets / Snapshots / History panel, then the photo with
// its toolbar under it, and the filmstrip along the bottom.

import { useEffect, useState } from 'react';

import { connectEngine, type Connection } from './engine/client';
import { text } from './i18n';
import { fileName } from './paths';
import { Filmstrip, StatusLine } from './layout/Filmstrip';
import { Histogram } from './layout/Histogram';
import { Navigator } from './layout/Navigator';
import { Rail } from './layout/Rail';
import { SideTabs } from './layout/SideTabs';
import { Toolbar } from './layout/Toolbar';
import { Viewer } from './layout/Viewer';
import { PanelStack } from './panels/PanelStack';
import { ShellProvider, useShell } from './ShellContext';
import { createShellStore, type ShellStore } from './store';
import './shell.css';

function shortcuts(store: ShellStore) {
  return (e: KeyboardEvent) => {
    const target = e.target as HTMLElement | null;
    if (target?.closest('input, select, textarea')) return;
    const s = store.getState();
    const mod = e.ctrlKey || e.metaKey;
    if (mod && e.key.toLowerCase() === 'z') {
      e.preventDefault();
      if (e.shiftKey) s.redo();
      else s.undo();
    } else if (mod && e.key.toLowerCase() === 'y') {
      e.preventDefault();
      s.redo();
    } else if (!mod && e.key.toLowerCase() === 'z') {
      s.setZoom(s.zoom === 'fit' ? 1 : 'fit');
    }
  };
}

export function ShellView({ store, fake }: { store: ShellStore; fake: boolean }) {
  const name = useShell((s) => (s.path ? fileName(s.path) : ''));
  useEffect(() => {
    void store.getState().init();
    const onKey = shortcuts(store);
    window.addEventListener('keydown', onKey);
    return () => {
      window.removeEventListener('keydown', onKey);
      store.getState().dispose();
    };
  }, [store]);

  return (
    <div className="ags">
      <header className="ags-top">
        <span className="ags-brand">{text('shell.brand')}</span>
        <span className="ags-top-name">{name}</span>
        {fake && (
          <span className="ags-badge" title={text('shell.fakeEngineHint')}>
            {text('shell.fakeEngine')}
          </span>
        )}
      </header>
      <main className="ags-main">
        <div className="ags-photo">
          <Viewer />
          <Toolbar />
        </div>
        <SideTabs />
        <aside className="ags-tools">
          <div className="ags-panel-block">
            <div className="ags-block-head">
              <span>{text('shell.histogram')}</span>
            </div>
            <Histogram />
          </div>
          <Navigator />
          <div className="ags-tools-scroll">
            <PanelStack />
          </div>
        </aside>
        <Rail />
      </main>
      <footer className="ags-bottom">
        <StatusLine fake={fake} />
        <Filmstrip />
      </footer>
    </div>
  );
}

function Connected({ connection }: { connection: Connection }) {
  const [store] = useState(() => createShellStore(connection.client, { photos: connection.photos }));
  return (
    <ShellProvider store={store}>
      <ShellView store={store} fake={connection.fake} />
    </ShellProvider>
  );
}

export default function Shell() {
  const [connection, setConnection] = useState<Connection | null>(null);
  useEffect(() => {
    void connectEngine().then(setConnection);
  }, []);
  return connection ? <Connected connection={connection} /> : <div className="ags" />;
}
