// The slim rail at the window's right edge, after RAWmakase's develop rail
// (src/app/side_tabs.rs, MIT): the tabs of the panel beside the photo. The
// open tab's button folds that panel; any other opens it. Tool modes (crop,
// spot removal, masking) join it when a tool declares one.

import { Bookmark, Camera, History } from 'lucide-react';
import type { ComponentType } from 'react';

import { text } from '../i18n';
import { useShell, useShellStore } from '../ShellContext';
import type { SideTab } from '../store';

const TABS: readonly (readonly [SideTab, ComponentType<{ size?: number }>])[] = [
  ['presets', Bookmark],
  ['snapshots', Camera],
  ['history', History],
];

export function Rail() {
  const store = useShellStore();
  const tab = useShell((s) => s.sideTab);
  return (
    <nav className="ags-rail-bar">
      {TABS.map(([t, Icon]) => (
        <button
          key={t}
          type="button"
          className={t === tab ? 'is-on' : ''}
          title={text(`shell.rail.${t}`)}
          aria-pressed={t === tab}
          onClick={() => store.getState().setSideTab(t === tab ? null : t)}
        >
          <Icon size={16} />
        </button>
      ))}
    </nav>
  );
}
