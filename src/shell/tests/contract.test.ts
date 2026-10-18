// The shell and its fake against the engine's own contract fixtures
// (src/shell/contract.fixtures.json, generated from contract.rs).

import { describe, expect, it } from 'vitest';

import fixtures from '../contract.fixtures.json';
import type { EditDocument, EditPatch, Event, ToolDescriptor } from '../contract';
import { applyPatch } from '../edit';
import { createFakeEngine } from '../engine/fake';
import { openShell, settle } from './helpers';

const tools = fixtures.tools as ToolDescriptor[];

describe('the contract fixtures', () => {
  it('are the sections and tools the fake describes', async () => {
    expect(await createFakeEngine().sections()).toEqual(fixtures.sections);
    expect(await createFakeEngine().tools()).toEqual(tools);
  });

  it('give the edit a photo opens with', async () => {
    const opened = await createFakeEngine().open('samples/A.CR3');
    expect(opened.edit).toEqual(fixtures.opened.edit);
    expect(opened.edit).toEqual(fixtures.edit);
  });

  it('patch the same way in the shell and in the fake, null putting a default back', async () => {
    const engine = createFakeEngine();
    const { photoId } = await engine.open('samples/A.CR3');
    await engine.apply(photoId, { rt_capture_sharpen: { radius: 1.5 } });
    const before = structuredClone(engine.editOf(photoId)!);
    await engine.apply(photoId, fixtures.patch as EditPatch);
    const shell = applyPatch(before, fixtures.patch as EditPatch, tools);
    expect(engine.editOf(photoId)).toEqual(shell);
    expect(shell.tools.ag_exposure_wb.settings).toMatchObject({ exposure: 0.5, red: 1.05 });
    expect(shell.tools.rt_capture_sharpen.settings.radius).toBe(0.75);
  });

  it('refuse a bad patch as a whole', async () => {
    const engine = createFakeEngine();
    const { photoId } = await engine.open('samples/A.CR3');
    const before = structuredClone(engine.editOf(photoId)) as EditDocument;
    await expect(
      engine.apply(photoId, { ag_exposure_wb: { exposure: 1 }, rt_capture_sharpen: { nope: 1 } }),
    ).rejects.toThrow();
    await expect(engine.apply(photoId, { dt_highlights: { recover: 3 } })).rejects.toThrow();
    expect(engine.editOf(photoId)).toEqual(before);
  });

  it('events are read in the shapes the engine sends', async () => {
    const { store } = await openShell();
    const [frame, histogram, error] = fixtures.events as Event[];
    expect(frame.event).toBe('frame');
    const ticket = store.getState().latestTicket;
    const photoId = store.getState().photo!.id;
    store.getState().receive({ ...(histogram as Extract<Event, { event: 'histogram' }>), ticket: ticket + 1, photoId });
    expect(store.getState().histogram).toHaveLength(256);
    store.getState().receive(error);
    expect(store.getState().error).toBe('no GPU: no adapter');
    await settle();
  });
});
