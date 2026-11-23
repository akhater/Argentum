import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createBlobUrlSlot, revokeBlobUrl, revokeBlobUrlLater, revokeBlobUrls } from './objectUrl';

let live: Set<string>;
let counter = 0;

beforeEach(() => {
  vi.useFakeTimers();
  live = new Set();
  counter = 0;
  vi.stubGlobal('URL', {
    createObjectURL: () => {
      const url = `blob:test/${++counter}`;
      live.add(url);
      return url;
    },
    revokeObjectURL: (url: string) => {
      live.delete(url);
    },
  });
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

const blob = () => ({}) as Blob;

describe('object URL slot', () => {
  it('keeps one live URL through many replacements and none after release', () => {
    const slot = createBlobUrlSlot();
    for (let i = 0; i < 25; i++) slot.replace(blob());
    vi.advanceTimersByTime(1000);
    expect(live.size).toBe(1);
    slot.release();
    expect(live.size).toBe(0);
  });

  it('does not revoke the displayed URL before the delay', () => {
    const slot = createBlobUrlSlot();
    const first = slot.replace(blob())!;
    slot.replace(blob());
    expect(live.has(first)).toBe(true);
    vi.advanceTimersByTime(300);
    expect(live.has(first)).toBe(false);
  });

  it('replacing with null drops the URL', () => {
    const slot = createBlobUrlSlot();
    slot.replace(blob());
    expect(slot.replace(null)).toBeNull();
    vi.advanceTimersByTime(300);
    expect(live.size).toBe(0);
  });
});

describe('revoke helpers', () => {
  it('ignores null and non-blob URLs', () => {
    const spy = vi.spyOn(URL, 'revokeObjectURL');
    revokeBlobUrl(null);
    revokeBlobUrl('data:image/jpeg;base64,AA');
    revokeBlobUrlLater('asset://x');
    vi.advanceTimersByTime(1000);
    expect(spy).not.toHaveBeenCalled();
  });

  it('leaves a URL the cache protects, also when protection starts during the delay', () => {
    live.add('blob:a');
    live.add('blob:b');
    revokeBlobUrlLater('blob:a', () => true);
    let protectedNow = false;
    revokeBlobUrlLater('blob:b', () => protectedNow);
    protectedNow = true;
    vi.advanceTimersByTime(1000);
    expect(live.size).toBe(2);
  });

  it('revokes every URL of a record', () => {
    live.add('blob:a');
    live.add('blob:b');
    revokeBlobUrls(Object.values({ x: 'blob:a', y: 'blob:b', z: null }));
    expect(live.size).toBe(0);
  });
});
