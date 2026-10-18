// Every change is a patch to the engine; undo and redo send inverse patches.

import { describe, expect, it } from 'vitest';

import { valueOf } from '../edit';
import { applies, openShell, settle } from './helpers';

const exposure = { key: 'exposure', default: 0 } as Parameters<typeof valueOf>[2];

describe('patches and undo', () => {
  it('send one small patch per change', async () => {
    const { engine, store } = await openShell();
    store.getState().setSetting('ag_exposure_wb', 'exposure', 0.5);
    store.getState().setSetting('dt_highlights', 'recover', false);
    await settle();
    expect(applies(engine)).toEqual([{ ag_exposure_wb: { exposure: 0.5 } }, { dt_highlights: { recover: false } }]);
  });

  it('undo with the inverse patch and redo with the patch, without snapshots', async () => {
    const { engine, store } = await openShell();
    const s = store.getState;
    s().setSetting('ag_exposure_wb', 'exposure', 0.5);
    s().setSetting('ag_exposure_wb', 'exposure', 1);
    s().undo();
    expect(valueOf(s().doc, 'ag_exposure_wb', exposure)).toBe(0.5);
    s().undo();
    expect(valueOf(s().doc, 'ag_exposure_wb', exposure)).toBe(0);
    s().redo();
    await settle();
    expect(applies(engine).slice(-3)).toEqual([
      { ag_exposure_wb: { exposure: 0.5 } },
      { ag_exposure_wb: { exposure: 0 } },
      { ag_exposure_wb: { exposure: 0.5 } },
    ]);
    expect(engine.editOf(s().photo!.photoId)!.tools.ag_exposure_wb.settings.exposure).toBe(0.5);
    // The history holds patches, never documents.
    for (const entry of [...s().past, ...s().future]) {
      expect(Object.keys(entry.patch)).toEqual(['ag_exposure_wb']);
      expect(Object.keys(entry.inverse)).toEqual(['ag_exposure_wb']);
    }
  });

  it('make one drag one step, while every tick still reaches the engine', async () => {
    const { engine, store } = await openShell();
    const s = store.getState;
    for (const v of [0.1, 0.2, 0.3, 0.4]) s().setSetting('ag_exposure_wb', 'exposure', v, { gesture: true });
    s().endGesture();
    await settle();
    expect(s().past).toHaveLength(1);
    expect(applies(engine)).toHaveLength(4);
    s().undo();
    expect(valueOf(s().doc, 'ag_exposure_wb', exposure)).toBe(0);
  });

  it('jump through history as one patch', async () => {
    const { engine, store } = await openShell();
    const s = store.getState;
    s().setSetting('ag_exposure_wb', 'exposure', 0.5);
    s().setSetting('dt_highlights', 'recover', false);
    s().setSetting('ag_exposure_wb', 'exposure', 2);
    const before = applies(engine).length;
    s().travel(0);
    await settle();
    expect(applies(engine).slice(before)).toEqual([
      { ag_exposure_wb: { exposure: 0 }, dt_highlights: { recover: true } },
    ]);
    expect(s().future).toHaveLength(3);
    s().travel(2);
    expect(valueOf(s().doc, 'ag_exposure_wb', exposure)).toBe(0.5);
  });

  it('reset a tool with nulls, as one step', async () => {
    const { engine, store } = await openShell();
    const s = store.getState;
    s().setSetting('rt_capture_sharpen', 'captureAmount', 40);
    s().setSetting('rt_capture_sharpen', 'capture', false);
    s().resetTool('rt_capture_sharpen');
    await settle();
    expect(applies(engine).at(-1)).toEqual({ rt_capture_sharpen: { captureAmount: null, capture: null } });
    expect(engine.editOf(s().photo!.photoId)!.tools.rt_capture_sharpen.settings).toMatchObject({
      captureAmount: 100,
      capture: true,
    });
    s().undo();
    expect(s().doc.tools.rt_capture_sharpen.settings).toMatchObject({ captureAmount: 40, capture: false });
  });

  it('roll back a change the engine refuses, and only that change', async () => {
    const { store } = await openShell({
      client: (engine) => ({
        ...engine,
        apply: (photo, patch) =>
          patch.rt_capture_sharpen
            ? Promise.reject({ kind: 'badValue', message: 'refused' })
            : engine.apply(photo, patch),
      }),
    });
    const s = store.getState;
    s().setSetting('ag_exposure_wb', 'exposure', 0.5);
    s().setSetting('rt_capture_sharpen', 'captureAmount', 40);
    expect(s().doc.tools.rt_capture_sharpen.settings.captureAmount).toBe(40);
    await settle();
    expect(s().doc.tools.rt_capture_sharpen.settings.captureAmount).toBe(100);
    expect(s().doc.tools.ag_exposure_wb.settings.exposure).toBe(0.5);
    expect(s().past).toHaveLength(1);
    expect(s().error).toBe('refused');
  });

  it('show the edit as the engine took it, not as it was sent', async () => {
    const { store } = await openShell();
    const s = store.getState;
    s().setSetting('ag_exposure_wb', 'exposure', -20);
    expect(s().doc.tools.ag_exposure_wb.settings.exposure).toBe(-20);
    await settle();
    expect(s().doc.tools.ag_exposure_wb.settings.exposure).toBe(-10);
    expect(s().confirmed).toEqual(s().doc);
  });

  it('apply presets and snapshots as patches', async () => {
    const { engine, store } = await openShell();
    const s = store.getState;
    s().setSetting('ag_exposure_wb', 'exposure', 1);
    s().savePreset('Bright');
    s().addSnapshot('One');
    s().setSetting('ag_exposure_wb', 'exposure', -1);
    s().setSetting('dt_highlights', 'recover', false);
    s().applySnapshot(0);
    await settle();
    expect(applies(engine).at(-1)).toEqual({ ag_exposure_wb: { exposure: 1 }, dt_highlights: { recover: true } });
    s().setSetting('ag_exposure_wb', 'exposure', 0);
    s().applyPreset(0);
    await settle();
    expect(applies(engine).at(-1)).toEqual({ ag_exposure_wb: { exposure: 1 } });
  });
});
