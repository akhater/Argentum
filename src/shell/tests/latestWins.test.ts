// Renders are latest-wins, and only of the photo shown: nothing for an older
// ticket or another photo is ever shown.

import { describe, expect, it } from 'vitest';

import { HISTOGRAM_BINS } from '../contract';
import type { EngineClient } from '../engine/client';
import { createFakeEngine, type FakeEngine } from '../engine/fake';
import { createShellStore } from '../store';
import { openShell, settle } from './helpers';

/** Record which tickets the store asks pixels for. */
function watchFetches(engine: FakeEngine) {
  const fetched: number[] = [];
  const frame = engine.frame.bind(engine);
  engine.frame = (t) => {
    fetched.push(t);
    return frame(t);
  };
  return fetched;
}

describe('latest wins', () => {
  it('shows the frame of the newest request after a burst of them', async () => {
    const { engine, store } = await openShell();
    const s = store.getState;
    for (const v of [0.1, 0.2, 0.3, 0.4, 0.5]) s().setSetting('ag_exposure', 'exposure_ev', v, { gesture: true });
    await settle();
    const newest = Math.max(...engine.calls.flatMap((c) => ('ticket' in c ? [c.ticket] : [])));
    expect(s().frame?.ticket).toBe(newest);
  });

  it('ignores a frame and a histogram older than one already seen, without fetching its pixels', async () => {
    const { engine, store } = await openShell();
    const s = store.getState;
    const fetched = watchFetches(engine);
    const shown = s().frame!.ticket;
    const bins = s().histogram;
    const photoId = s().photo!.photoId;
    s().receive({
      event: 'frame',
      ticket: shown - 1,
      photoId,
      placement: { xPx: 0, yPx: 0, widthPx: 800, heightPx: 600 },
    });
    s().receive({ event: 'histogram', ticket: shown - 1, photoId, bins: new Array(HISTOGRAM_BINS).fill(9) });
    await settle();
    expect(s().frame!.ticket).toBe(shown);
    expect(s().histogram).toBe(bins);
    expect(fetched).toEqual([]);
  });

  it('drops a frame whose pixels arrive after a newer frame was announced', async () => {
    const engine = createFakeEngine({ autoFrames: false });
    const waiting = new Map<number, () => void>();
    const pixels = { width: 8, height: 8, data: new Uint8ClampedArray(8 * 8 * 4) };
    const slow: EngineClient = {
      ...engine,
      frame: (t) => new Promise((resolve) => waiting.set(t, () => resolve(pixels))),
    };
    const store = createShellStore(slow, { photos: ['samples/A.CR3'] });
    await store.getState().init();
    store.getState().setViewSize({ width_px: 8, height_px: 8 });
    const photoId = store.getState().photo!.photoId;
    store
      .getState()
      .receive({ event: 'frame', ticket: 5, photoId, placement: { xPx: 0, yPx: 0, widthPx: 8, heightPx: 8 } });
    store
      .getState()
      .receive({ event: 'frame', ticket: 6, photoId, placement: { xPx: 0, yPx: 0, widthPx: 8, heightPx: 8 } });
    waiting.get(5)!();
    await settle();
    expect(store.getState().frame).toBeNull();
    waiting.get(6)!();
    await settle();
    expect(store.getState().frame?.ticket).toBe(6);
  });
});

describe('only the photo shown', () => {
  it('drops a frame and a histogram of another photo, however new their ticket', async () => {
    const { engine, store } = await openShell();
    const s = store.getState;
    const fetched = watchFetches(engine);
    const shown = s().frame!.ticket;
    const bins = s().histogram;
    const other = s().photo!.photoId + 1;
    s().receive({
      event: 'frame',
      ticket: shown + 10,
      photoId: other,
      placement: { xPx: 0, yPx: 0, widthPx: 800, heightPx: 600 },
    });
    s().receive({ event: 'histogram', ticket: shown + 10, photoId: other, bins: new Array(HISTOGRAM_BINS).fill(9) });
    await settle();
    expect(s().frame!.ticket).toBe(shown);
    expect(s().histogram).toBe(bins);
    expect(s().latestTicket).toBe(shown);
    expect(fetched).toEqual([]);
  });

  it('does not show a late frame of the last photo on the next one', async () => {
    const { engine, store } = await openShell();
    const s = store.getState;
    const before = s().photo!.photoId;
    await s().openPhoto('samples/B.CR3');
    await settle();
    const fetched = watchFetches(engine);
    const shown = s().frame!.ticket;
    s().receive({
      event: 'frame',
      ticket: shown + 1,
      photoId: before,
      placement: { xPx: 0, yPx: 0, widthPx: 800, heightPx: 600 },
    });
    await settle();
    expect(s().photo!.photoId).not.toBe(before);
    expect(s().frame!.ticket).toBe(shown);
    expect(fetched).toEqual([]);
  });
});
