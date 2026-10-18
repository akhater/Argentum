import { createContext, useContext, useSyncExternalStore, type ReactNode } from 'react';

import type { Shell, ShellStore } from './store';

const ShellContext = createContext<ShellStore | null>(null);

export function ShellProvider({ store, children }: { store: ShellStore; children: ReactNode }) {
  return <ShellContext.Provider value={store}>{children}</ShellContext.Provider>;
}

export function useShellStore(): ShellStore {
  const store = useContext(ShellContext);
  if (!store) throw new Error('useShell outside ShellProvider');
  return store;
}

/**
 * Read the store through a selector, re-rendering only when what it returns
 * changes. Return a value or a reference the store keeps, never a new object.
 * The current state also serves a render to a string, as the tests do.
 */
export function useShell<T>(selector: (s: Shell) => T): T {
  const store = useShellStore();
  const read = () => selector(store.getState());
  return useSyncExternalStore(store.subscribe, read, read);
}
