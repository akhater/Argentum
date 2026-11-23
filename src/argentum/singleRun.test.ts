import { describe, expect, it, vi } from 'vitest';
import { onceGate, unlessRunning } from './singleRun';

describe('onceGate', () => {
  it('lets the first claim through and refuses the rest', () => {
    const gate = onceGate();
    expect([gate.claim(), gate.claim(), gate.claim()]).toEqual([true, false, false]);
  });

  it('lets one claim through again after a reset (the next opening)', () => {
    const gate = onceGate();
    gate.claim();
    gate.reset();
    expect(gate.claim()).toBe(true);
  });
});

describe('unlessRunning', () => {
  it('starts one run for two triggers while the first is pending', async () => {
    let finish!: () => void;
    const run = vi.fn(() => new Promise<void>((resolve) => (finish = resolve)));
    const first = unlessRunning('k', run);
    const second = unlessRunning('k', run);
    expect(run).toHaveBeenCalledTimes(1);
    finish();
    await first;
    await second;
  });

  it('runs again once the first has finished, even when it failed', async () => {
    const run = vi.fn().mockRejectedValueOnce(new Error('boom')).mockResolvedValue(undefined);
    await expect(unlessRunning('k2', run)).rejects.toThrow('boom');
    await unlessRunning('k2', run);
    expect(run).toHaveBeenCalledTimes(2);
  });

  it('keeps different keys apart', async () => {
    let finish!: () => void;
    const slow = vi.fn(() => new Promise<void>((resolve) => (finish = resolve)));
    const other = vi.fn().mockResolvedValue(undefined);
    const first = unlessRunning('a', slow);
    await unlessRunning('b', other);
    expect(other).toHaveBeenCalledTimes(1);
    finish();
    await first;
  });
});
