// The saves that write to disk start from timers and nobody waits for them. Closing the window has
// to: it flushes the timers, then waits here for the writes they started.
const open = new Set<Promise<unknown>>();

/** Remembers a write until it settles; hands the same promise back. */
export function trackSave<T>(write: Promise<T>): Promise<T> {
  open.add(write);
  const done = () => open.delete(write);
  write.then(done, done);
  return write;
}

/** Settles once every write remembered so far, and any started while waiting, has finished or failed. */
export async function savesSettled(): Promise<void> {
  while (open.size > 0) await Promise.allSettled([...open]);
}
