// The new editor interface, in two modes. Browse: folders and a grid of their
// photos (browse/). Edit: layout after RAWmakase 0.3.3's Develop
// (src/app/workspace.rs and side_tabs.rs @ 7c3c81ab, MIT), itself Lightroom Classic's:
// from the window's right edge the rail, the tool panels under the histogram
// and navigator, the Presets / Snapshots / History panel, then the photo with
// its toolbar under it, and the filmstrip along the bottom.

import { useEffect, useState } from 'react';

import { BrowseView } from './browse/BrowseView';
import { MODE_KEYS, ModeSwitch } from './browse/ModeSwitch';
import { AppBar } from './engine/AppBar';
import { connectEngine, type AppConnection, type Connection } from './engine/client';
import { text } from './i18n';
import { fileName } from './paths';
import { Filmstrip, StatusLine } from './layout/Filmstrip';
import { Histogram } from './layout/Histogram';
import { Navigator } from './layout/Navigator';
import { PanelBlock } from './layout/PanelBlock';
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
    } else if (!mod && e.key.toLowerCase() === MODE_KEYS.browse) {
      s.setMode('browse');
    } else if (!mod && e.key.toLowerCase() === MODE_KEYS.edit) {
      s.setMode('edit');
    } else if (!mod && e.key.toLowerCase() === 'z' && s.mode === 'edit') {
      s.setZoom(s.zoom === 'fit' ? 1 : 'fit');
    }
  };
}

export function ShellView({ store, fake, app }: { store: ShellStore; fake: boolean; app?: AppConnection }) {
  const name = useShell((s) => (s.mode === 'edit' && s.path ? fileName(s.path) : ''));
  const mode = useShell((s) => s.mode);
  useEffect(() => {
    void store.getState().init();
    void store.getState().initBrowse();
    const onKey = shortcuts(store);
    window.addEventListener('keydown', onKey);
    return () => {
      window.removeEventListener('keydown', onKey);
      store.getState().dispose();
      store.getState().disposeBrowse();
    };
  }, [store]);

  return (
    <div className="ags">
      <header className="ags-top">
        <span className="ags-brand">{text('shell.brand')}</span>
        <ModeSwitch />
        <span className="ags-top-name">{name}</span>
        {app && <AppBar app={app} />}
        {fake && (
          <span className="ags-badge" title={text('shell.fakeEngineHint')}>
            {text('shell.fakeEngine')}
          </span>
        )}
      </header>
      <main className="ags-main">{mode === 'browse' ? <BrowseView /> : <EditView />}</main>
      {mode === 'edit' && (
        <footer className="ags-bottom">
          <StatusLine fake={fake} />
          <Filmstrip />
        </footer>
      )}
    </div>
  );
}

function EditView() {
  return (
    <>
      <div className="ags-photo">
        <Viewer />
        <Toolbar />
      </div>
      <SideTabs />
      <aside className="ags-tools">
        <PanelBlock part="histogram" title={text('shell.histogram')}>
          <Histogram />
        </PanelBlock>
        <Navigator />
        <div className="ags-tools-scroll">
          <PanelStack />
        </div>
      </aside>
      <Rail />
    </>
  );
}

function Connected({ connection }: { connection: Connection }) {
  const [store] = useState(() => createShellStore(connection.client, { browse: connection.browse }));
  return (
    <ShellProvider store={store}>
      <ShellView store={store} fake={connection.fake} app={connection.app} />
    </ShellProvider>
  );
}

export default function Shell() {
  const [connection, setConnection] = useState<Connection | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    connectEngine().then(
      (c) => live && setConnection(c),
      (e: unknown) => live && setFailure(typeof e === 'object' && e && 'message' in e ? String(e.message) : String(e)),
    );
    return () => {
      live = false;
    };
  }, []);
  if (connection) return <Connected connection={connection} />;
  return (
    <div className="ags">
      {failure && (
        <p className="ags-error" role="alert">
          {text('shell.notConnected', { message: failure })}
        </p>
      )}
    </div>
  );
}
