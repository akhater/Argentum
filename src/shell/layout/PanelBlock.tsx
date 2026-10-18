// A panel above the tools, the histogram or the navigator: its title folds it,
// as a tool section's does, and the store keeps which are folded.

import { ChevronDown, ChevronRight } from 'lucide-react';
import type { ReactNode } from 'react';

import { useShell, useShellStore } from '../ShellContext';
import type { Foldable } from '../store';

export function PanelBlock({
  part,
  title,
  controls,
  children,
}: {
  part: Foldable;
  title: string;
  /** Shown beside the title while the panel is open. */
  controls?: ReactNode;
  children: ReactNode;
}) {
  const store = useShellStore();
  const folded = useShell((s) => s.collapsed[part] === true);
  return (
    <div className="ags-panel-block" data-panel={part}>
      <div className="ags-block-head">
        <button
          type="button"
          className="ags-block-toggle"
          aria-expanded={!folded}
          onClick={() => store.getState().toggleFolded(part)}
        >
          {folded ? <ChevronRight size={13} /> : <ChevronDown size={13} />}
          <span>{title}</span>
        </button>
        {!folded && controls}
      </div>
      {!folded && children}
    </div>
  );
}
