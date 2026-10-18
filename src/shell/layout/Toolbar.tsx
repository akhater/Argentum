// Under the photo, on its backdrop, as in RAWmakase's Develop toolbar
// (src/app/toolbar.rs, MIT): undo and redo, the zoom, and Reset all.

import { Redo2, RotateCcw, Undo2 } from 'lucide-react';

import { resetPatch } from '../edit';
import { text } from '../i18n';
import { useShell, useShellStore } from '../ShellContext';
import { fitZoom } from '../store';

export function Toolbar() {
  const store = useShellStore();
  const canUndo = useShell((s) => s.past.length > 0);
  const canRedo = useShell((s) => s.future.length > 0);
  const percent = useShell((s) => Math.round((s.zoom === 'fit' ? fitZoom(s) : s.zoom) * 100));
  const hasPhoto = useShell((s) => s.photo !== null);

  return (
    <div className="ags-toolbar">
      <button
        type="button"
        disabled={!canUndo}
        title={`${text('shell.undo')} · Ctrl+Z`}
        onClick={() => store.getState().undo()}
      >
        <Undo2 size={15} />
      </button>
      <button
        type="button"
        disabled={!canRedo}
        title={`${text('shell.redo')} · Ctrl+Shift+Z`}
        onClick={() => store.getState().redo()}
      >
        <Redo2 size={15} />
      </button>
      <span className="ags-toolbar-divider" />
      <span className="ags-toolbar-zoom">{hasPhoto ? `${percent}%` : ''}</span>
      <span className="ags-toolbar-spacer" />
      <button
        type="button"
        disabled={!hasPhoto}
        onClick={() => {
          const s = store.getState();
          s.commit(resetPatch(s.doc, s.tools), { kind: 'resetAll' });
        }}
      >
        <RotateCcw size={13} />
        <span>{text('shell.resetAll')}</span>
      </button>
    </div>
  );
}
