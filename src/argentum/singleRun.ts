/**
 * Run an action once, not twice. Ours.
 *
 * `onceGate`: one claim per opening (a confirm dialog's action). `unlessRunning`:
 * a second trigger while the first run is still pending does nothing (Paste
 * files, Generate), and the next one works as soon as the first has finished,
 * failed or not.
 */

export function onceGate() {
  let used = false;
  return {
    /** True for the first call only, until `reset`. */
    claim(): boolean {
      if (used) return false;
      used = true;
      return true;
    },
    reset(): void {
      used = false;
    },
  };
}

const running = new Set<string>();

/** Runs `run` unless a run under the same `key` is still pending; then resolves undefined at once. */
export async function unlessRunning<T>(key: string, run: () => Promise<T>): Promise<T | undefined> {
  if (running.has(key)) return undefined;
  running.add(key);
  try {
    return await run();
  } finally {
    running.delete(key);
  }
}
