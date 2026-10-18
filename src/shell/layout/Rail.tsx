// The slim rail at the window's right edge, after RAWmakase's develop rail
// (src/app/side_tabs.rs, MIT): first the modes, which switch the panel between
// the tools and a tool's drawer, then the tabs of the panel beside the photo.
// The open tab's button folds that panel; any other opens it.

import { Bandage, Bookmark, Camera, Crop, History, SlidersHorizontal, SquareDashedMousePointer } from 'lucide-react';
import type { ComponentType } from 'react';

import { text } from '../i18n';
import { useShell, useShellStore } from '../ShellContext';
import type { Mode, SideTab } from '../store';

type Icon = ComponentType<{ size?: number }>;

const MODES: [Mode, Icon][] = [
  ['edit', SlidersHorizontal],
  ['crop', Crop],
  ['spot', Bandage],
  ['masking', SquareDashedMousePointer],
];

const TABS: [SideTab, Icon][] = [
  ['presets', Bookmark],
  ['snapshots', Camera],
  ['history', History],
];

export function Rail() {
  const store = useShellStore();
  const mode = useShell((s) => s.mode);
  const tab = useShell((s) => s.sideTab);
  return (
    <nav className="ags-rail-bar">
      {MODES.map(([m, Icon]) => (
        <button
          key={m}
          type="button"
          className={m === mode ? 'is-on' : ''}
          title={text(`shell.rail.${m}`)}
          aria-pressed={m === mode}
          onClick={() => store.getState().setMode(m)}
        >
          <Icon size={17} />
        </button>
      ))}
      <span className="ags-rail-gap" />
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
