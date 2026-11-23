// Closing the window with a change still waiting in a timer.
import { beforeEach, describe, expect, it, vi } from 'vitest';

const log = vi.hoisted(() => [] as string[]);
const hold = vi.hoisted(() => ({ mode: 'ok' as 'ok' | 'reject' | 'never' }));

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (command: string, args?: { path?: string }) => {
    if (command === 'is_image_cached') return Promise.resolve(true);
    log.push(`${command}${args?.path ? `:${args.path}` : ''}`);
    if (hold.mode === 'never') return new Promise(() => {});
    if (hold.mode === 'reject') return Promise.reject(new Error('disk full'));
    return new Promise((resolve) =>
      setTimeout(() => {
        log.push(`done:${command}`);
        resolve(undefined);
      }, 20),
    );
  },
}));

// What Tauri's onCloseRequested does: run the handler, then destroy the window unless prevented.
vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({
    onCloseRequested: async (handler: (e: { preventDefault: () => void }) => Promise<void>) => {
      (globalThis as { __close?: () => Promise<void> }).__close = async () => {
        await handler({ preventDefault: () => {} });
        log.push('destroy');
      };
      return () => {};
    },
  }),
}));

import { useEditorStore } from '../store/useEditorStore';
import { INITIAL_ADJUSTMENTS } from '../utils/adjustments';
import { debouncedSetHistory } from './historyOnRelease';
import { savePresetsSoon } from '../hooks/usePresets';
import { listenForClose } from './saveOnClose';
import type { QueuedEdit } from './pendingSave';

const close = async () => {
  await listenForClose({ current: queued });
  await (globalThis as unknown as { __close: () => Promise<void> }).__close();
};
let queued: QueuedEdit | null = null;

describe('closing the window', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    log.length = 0;
    hold.mode = 'ok';
    debouncedSetHistory.cancel();
    queued = { path: 'a.cr3', adjustments: INITIAL_ADJUSTMENTS };
    useEditorStore.setState({
      selectedImage: { path: 'a.cr3', isReady: true } as never,
      adjustments: INITIAL_ADJUSTMENTS,
      previewOverride: null,
    });
  });

  it('saves the open photo before the window is destroyed', async () => {
    useEditorStore.setState({ adjustments: { ...INITIAL_ADJUSTMENTS, rotation: 3 } });
    const closing = close();
    await vi.runAllTimersAsync();
    await closing;
    expect(log.indexOf('save_metadata_and_update_thumbnail:a.cr3')).toBeGreaterThanOrEqual(0);
    expect(log.indexOf('done:save_metadata_and_update_thumbnail')).toBeLessThan(log.indexOf('destroy'));
  });

  it('writes the last preset change before the window is destroyed', async () => {
    savePresetsSoon([]);
    const closing = close();
    await vi.runAllTimersAsync();
    await closing;
    expect(log.indexOf('done:save_presets')).toBeGreaterThanOrEqual(0);
    expect(log.indexOf('done:save_presets')).toBeLessThan(log.indexOf('destroy'));
  });

  it('records the last History step and writes it before the window is destroyed', async () => {
    const edited = { ...INITIAL_ADJUSTMENTS, rotation: 3 };
    useEditorStore.setState({ adjustments: edited });
    debouncedSetHistory(edited);
    const closing = close();
    await vi.runAllTimersAsync();
    await closing;
    expect(useEditorStore.getState().history.at(-1)).toBe(edited);
    expect(log.indexOf('destroy')).toBe(log.length - 1);
  });

  it('closes at once, with no save, when nothing is pending', async () => {
    const closing = close();
    await closing;
    expect(log).toEqual(['destroy']);
  });

  it('still closes when the save fails', async () => {
    hold.mode = 'reject';
    useEditorStore.setState({ adjustments: { ...INITIAL_ADJUSTMENTS, rotation: 3 } });
    const closing = close();
    await vi.runAllTimersAsync();
    await closing;
    expect(log.at(-1)).toBe('destroy');
  });

  it('still closes, within the bound, when the save never settles', async () => {
    hold.mode = 'never';
    useEditorStore.setState({ adjustments: { ...INITIAL_ADJUSTMENTS, rotation: 3 } });
    let closed = false;
    const closing = close().then(() => (closed = true));
    await vi.advanceTimersByTimeAsync(1900);
    expect(closed).toBe(false);
    await vi.advanceTimersByTimeAsync(200);
    await closing;
    expect(log.at(-1)).toBe('destroy');
  });
});
