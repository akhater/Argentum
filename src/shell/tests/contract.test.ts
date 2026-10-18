// The shell and its fake against the engine's own contract fixtures
// (src/shell/contract.fixtures.json, generated from contract.rs): the fake
// takes and refuses exactly what the engine does.

import { describe, expect, it } from 'vitest';

import fixtures from '../contract.fixtures.json';
import {
  HISTOGRAM_BINS,
  type Applied,
  type EditDocument,
  type EditPatch,
  type EngineError,
  type Event,
} from '../contract';
import { createFakeEngine } from '../engine/fake';
import { openShell, settle } from './helpers';

// JSON imports are typed by their contents; these are the contract's shapes.
const patched = fixtures.patched as unknown as { patch: EditPatch; edit: EditDocument }[];
const refused = fixtures.refused as unknown as { patch: EditPatch; error: EngineError }[];

/** A fake with one photo open, at the defaults. */
async function opened() {
  const engine = createFakeEngine();
  const { photoId } = await engine.open('samples/A.CR3');
  return { engine, photoId };
}

describe('the contract fixtures', () => {
  it('are the sections and tools the fake describes', async () => {
    expect(await createFakeEngine().sections()).toEqual(fixtures.sections);
    expect(await createFakeEngine().tools()).toEqual(fixtures.tools);
  });

  it('give the edit a photo opens with', async () => {
    const { engine, photoId } = await opened();
    expect(engine.editOf(photoId)).toEqual(fixtures.opened.edit);
    expect(fixtures.opened.edit).toEqual(fixtures.edit);
  });

  it('apply: the edit back is the engine’s, defaults filled', async () => {
    const { engine, photoId } = await opened();
    const applied: Applied = await engine.apply(photoId, fixtures.patch as EditPatch);
    expect(applied.edit).toEqual((fixtures.applied as Applied).edit);
  });

  it('patched: the fake clamps and fills in exactly as the engine', async () => {
    expect(patched.length).toBeGreaterThan(0);
    for (const { patch, edit } of patched) {
      const { engine, photoId } = await opened();
      expect((await engine.apply(photoId, patch)).edit, JSON.stringify(patch)).toEqual(edit);
    }
  });

  it('refused: the fake refuses exactly what the engine refuses, as a whole, with its kind', async () => {
    expect(refused.length).toBeGreaterThan(0);
    for (const { patch, error } of refused) {
      const { engine, photoId } = await opened();
      const refusal = await engine.apply(photoId, patch).then(
        () => null,
        (e: EngineError) => e,
      );
      expect(refusal?.kind, JSON.stringify(patch)).toBe(error.kind);
      expect(engine.editOf(photoId), JSON.stringify(patch)).toEqual(fixtures.edit);
    }
  });

  it('events are read in the shapes the engine sends', async () => {
    const { store } = await openShell();
    const [frame, histogram, error] = fixtures.events as Event[];
    expect(frame.event).toBe('frame');
    const ticket = store.getState().latestTicket;
    const photoId = store.getState().photo!.photoId;
    store.getState().receive({ ...(histogram as Extract<Event, { event: 'histogram' }>), ticket: ticket + 1, photoId });
    expect(store.getState().histogram).toHaveLength(HISTOGRAM_BINS);
    store.getState().receive(error);
    expect(store.getState().error).toBe('no GPU: no adapter');
    await settle();
  });
});
