// A presets file that could not be loaded must never be saved over: the list is "could not load",
// not "empty", until a later load succeeds (hooks/usePresets.ts).
import { beforeEach, describe, expect, it, vi } from 'vitest';

const backend = vi.hoisted(() => ({ calls: [] as string[], load: 'ok' as 'ok' | 'fail' }));

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (command: string) => {
    backend.calls.push(command);
    if (command === 'load_presets') {
      return backend.load === 'fail' ? Promise.reject(new Error('unreadable')) : Promise.resolve([]);
    }
    return Promise.resolve(undefined);
  },
}));
const toast = vi.hoisted(() => ({ error: vi.fn() }));
vi.mock('react-toastify', () => ({ toast }));

import { loadSavedPresets, savePresetsSoon } from '../hooks/usePresets';

const saves = () => backend.calls.filter((c) => c === 'save_presets').length;
const change = [{ preset: { id: 'a', name: 'New', adjustments: {} } }] as never;

beforeEach(() => {
  vi.useFakeTimers();
  backend.calls = [];
  toast.error.mockClear();
});

describe('presets that could not be loaded', () => {
  it('are never saved: a change after a failed load makes no save call and says why', async () => {
    backend.load = 'fail';
    await loadSavedPresets();

    savePresetsSoon(change);
    savePresetsSoon.flush();
    await vi.runAllTimersAsync();

    expect(saves()).toBe(0);
    expect(toast.error).toHaveBeenCalledTimes(1);
  });

  it('are saved again once a later load succeeds', async () => {
    backend.load = 'fail';
    await loadSavedPresets();
    backend.load = 'ok';
    await loadSavedPresets();

    savePresetsSoon(change);
    savePresetsSoon.flush();
    await vi.runAllTimersAsync();

    expect(saves()).toBe(1);
    expect(toast.error).not.toHaveBeenCalled();
  });
});
