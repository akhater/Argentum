/**
 * What the library shows in place of today's start screen while Modern fills
 * the folder column. Ours.
 *
 * The library shows its splash whenever it has no folders. Under Modern it
 * never needs one: this asks for the column once and shows nothing until it
 * arrives, a fraction of a second. If no disk can be listed, the library goes
 * back to the splash, so Argentum is never left blank.
 */

import { useEffect, useRef } from 'react';
import { useEditorLayout } from './editorLayout';
import { useModernFolders } from './modernFolders';

/** `unknown` while the layout preference is being read, so neither screen flashes. */
export type ModernStartState = 'unknown' | 'modern' | 'splash';

export function useModernStart(): ModernStartState {
  const layout = useEditorLayout((s) => s.layout);
  const unavailable = useModernFolders((s) => s.unavailable);
  if (layout === null) return 'unknown';
  return layout === 'modern' && !unavailable ? 'modern' : 'splash';
}

export function ModernStarting({ layoutKnown, onStart }: { layoutKnown: boolean; onStart(): void }) {
  const started = useRef(false);
  useEffect(() => {
    if (layoutKnown && !started.current) {
      started.current = true;
      onStart();
    }
  }, [layoutKnown, onStart]);
  return <div className="flex-1 h-full" />;
}
