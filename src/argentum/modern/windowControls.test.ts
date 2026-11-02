import { beforeEach, describe, expect, it, vi } from 'vitest';

/**
 * Tests have no DOM, so React runs no effects. `hook` stands in for the part that
 * matters here: state that stays between renders, and an effect that runs again,
 * cleaning up first, only when a dependency changes.
 */
const { hook, resize } = vi.hoisted(() => ({
  hook: {
    states: [] as unknown[],
    slot: 0,
    effect: undefined as { run: () => (() => void) | void; deps: unknown[] } | undefined,
  },
  resize: { listens: 0, stops: 0, resolve: [] as (() => void)[] },
}));

vi.mock('react', async (original) => ({
  ...(await original<typeof import('react')>()),
  useState: (initial: unknown) => {
    const slot = hook.slot++;
    if (!(slot in hook.states)) hook.states[slot] = typeof initial === 'function' ? initial() : initial;
    return [hook.states[slot], () => {}];
  },
  useCallback: (callback: unknown) => callback,
  useEffect: (run: () => (() => void) | void, deps: unknown[]) => {
    hook.effect = { run, deps };
  },
}));

// A new object from each call and a listener that takes a while to be in place,
// as the real ones do.
vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({
    isMaximized: async () => false,
    onResized: () => {
      resize.listens += 1;
      return new Promise((resolve) => {
        resize.resolve.push(() =>
          resolve(() => {
            resize.stops += 1;
          }),
        );
      });
    },
  }),
}));

vi.mock('@tauri-apps/plugin-os', () => ({ platform: () => 'windows' }));

import { useWindow } from './WindowControls';

/** Renders `useWindow` this many times and gives back its unmount. */
function render(times: number) {
  let deps: unknown[] | undefined;
  let cleanup: (() => void) | void = undefined;
  for (let i = 0; i < times; i += 1) {
    hook.slot = 0;
    useWindow('windows');
    const effect = hook.effect!;
    if (!deps || effect.deps.some((dep, at) => !Object.is(dep, deps![at]))) {
      cleanup?.();
      cleanup = effect.run();
    }
    deps = effect.deps;
  }
  return () => cleanup?.();
}

async function listenersComeUp() {
  resize.resolve.splice(0).forEach((resolve) => resolve());
  await Promise.resolve();
  await Promise.resolve();
}

describe('the window controls follow the window size', () => {
  beforeEach(() => {
    hook.states = [];
    resize.listens = 0;
    resize.stops = 0;
    resize.resolve = [];
  });

  // Every resize re-renders the top bar: a listener per render piled up on
  // each one, and each listener asks the window whether it is maximised.
  it('keeps one resize listener however often it renders', () => {
    render(50);
    expect(resize.listens).toBe(1);
  });

  it('stops a listener that was still being set up when it unmounted', async () => {
    const unmount = render(1);
    unmount();
    await listenersComeUp();
    expect(resize.stops).toBe(1);
  });

  it('stops its listener when it unmounts later', async () => {
    const unmount = render(1);
    await listenersComeUp();
    unmount();
    await Promise.resolve();
    await Promise.resolve();
    expect(resize.stops).toBe(1);
  });
});
