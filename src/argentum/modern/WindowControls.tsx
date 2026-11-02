/**
 * Minimise, maximise and close, at the right end of the Modern top bar, which
 * is also the window's title bar. Ours.
 *
 * Adapted from today's src/window/TitleBar.tsx (its handlers and the restore
 * icon): there the controls sit in a bar of their own; here the top bar is the
 * only one. On macOS the traffic lights go at the left end instead
 * (MacLights). With the system's own title bar on (Settings > General), none
 * of this is drawn.
 */

import { useCallback, useEffect, useState } from 'react';
import { platform } from '@tauri-apps/plugin-os';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { Minus, Square, X } from 'lucide-react';

const RestoreDown = () => (
  <svg width={12} height={12} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5">
    <rect x="3" y="6" width="8" height="8" rx="1.5" />
    <path d="M6 6V4.5A1.5 1.5 0 0 1 7.5 3h5A1.5 1.5 0 0 1 14 4.5v5A1.5 1.5 0 0 1 12.5 11H11" />
  </svg>
);

/** The platform, or '' until it is known. */
export function usePlatform(): string {
  const [os, setOs] = useState('');
  useEffect(() => {
    try {
      setOs(platform());
    } catch {
      setOs('windows');
    }
  }, []);
  return os;
}

export function useWindow(os: string) {
  // One object across renders: each call makes a new one, and the effect below
  // would listen again on every render, and a resize renders this.
  const [appWindow] = useState(getCurrentWindow);
  const [maximised, setMaximised] = useState(false);

  useEffect(() => {
    const read = () =>
      appWindow
        .isMaximized()
        .then(setMaximised)
        .catch(() => {});
    read();
    const listening = appWindow.onResized(read);
    return () => {
      listening.then((stop) => stop()); // through the promise: it may not be listening yet
    };
  }, [appWindow]);

  const toggle = useCallback(async () => {
    try {
      if (os === 'macos') {
        await appWindow.setFullscreen(!(await appWindow.isFullscreen()));
      } else {
        await appWindow.toggleMaximize();
      }
    } catch (err) {
      console.error('Failed to toggle maximize:', err);
    }
  }, [appWindow, os]);

  return { maximised, minimise: () => appWindow.minimize(), toggle, close: () => appWindow.close() };
}

export default function WindowControls({ os }: { os: string }) {
  const w = useWindow(os);
  if (!os || os === 'macos') return null;
  return (
    <div className="agm-wc">
      <button aria-label="Minimize window" onClick={w.minimise}>
        <Minus size={15} />
      </button>
      <button aria-label="Maximize window" onClick={w.toggle}>
        {w.maximised ? <RestoreDown /> : <Square size={11} />}
      </button>
      <button aria-label="Close window" className="agm-close" onClick={w.close}>
        <X size={15} />
      </button>
    </div>
  );
}

export function MacLights({ os }: { os: string }) {
  const w = useWindow(os);
  if (os !== 'macos') return null;
  return (
    <div className="flex items-center h-full px-3 gap-2">
      <button aria-label="Close window" className="w-3 h-3 rounded-full bg-red-500" onClick={w.close} />
      <button aria-label="Minimize window" className="w-3 h-3 rounded-full bg-yellow-500" onClick={w.minimise} />
      <button aria-label="Maximize window" className="w-3 h-3 rounded-full bg-green-500" onClick={w.toggle} />
    </div>
  );
}
