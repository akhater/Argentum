// Browse or Edit, in the top bar, with the keys Lightroom uses: G for the
// grid, D for develop.

import { LayoutGrid, SlidersHorizontal } from 'lucide-react';

import { text } from '../i18n';
import { useShell, useShellStore } from '../ShellContext';
import type { Mode } from './browseSlice';

/** The key that switches to each mode. */
export const MODE_KEYS: Record<Mode, string> = { browse: 'g', edit: 'd' };

export function ModeSwitch() {
  const store = useShellStore();
  const mode = useShell((s) => s.mode);
  const button = (to: Mode, Icon: typeof LayoutGrid) => (
    <button
      type="button"
      className={mode === to ? 'agb-mode is-on' : 'agb-mode'}
      aria-pressed={mode === to}
      title={text(`browse.mode.${to}Key`, { key: MODE_KEYS[to].toUpperCase() })}
      onClick={() => store.getState().setMode(to)}
    >
      <Icon size={14} />
      <span>{text(`browse.mode.${to}`)}</span>
    </button>
  );
  return (
    <div className="agb-modes" role="group">
      {button('browse', LayoutGrid)}
      {button('edit', SlidersHorizontal)}
    </div>
  );
}
