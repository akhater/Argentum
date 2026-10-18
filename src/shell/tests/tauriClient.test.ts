// The real client, as it runs inside the app, against the engine's contract
// fixtures and the app's wire fixtures (engine/wire.fixtures.json): the same
// calls and the same frame block the Rust side's tests read
// (src-tauri/src/mods/dispatch.rs, engine_frame.rs).

import { describe, expect, it } from 'vitest';

import fixtures from '../contract.fixtures.json';
import type { EditPatch, Event, ToolDescriptor } from '../contract';
import { showFolderOf } from '../engine/AppBar';
import { connectApp, createTauriEngine, ENGINE_EVENT, readFrame, type Transport } from '../engine/tauri';
import wire from '../engine/wire.fixtures.json';
import { createShellStore } from '../store';
import { settle } from './helpers';

const PATH = wire.calls.engine_open.path;

/** A frame's block: the header, then grey pixels. */
function block(ticket: number, width: number, height: number): ArrayBuffer {
  const bytes = new Uint8Array(12 + width * height * 4).fill(128);
  const head = new DataView(bytes.buffer);
  [ticket, width, height].forEach((v, i) => head.setUint32(i * 4, v, true));
  return bytes.buffer;
}

/**
 * The app as the client sees it: answers from the fixtures, every call
 * recorded. With `shared`, frames come as shared buffers, as on Windows.
 */
function app(options: { folder?: string[]; shared?: boolean; refuse?: unknown } = {}) {
  const calls: [string, Record<string, unknown>][] = [];
  const listeners = new Map<string, ((payload: unknown) => void)[]>();
  const sharedHandlers: ((buffer: ArrayBuffer, release: () => void) => void)[] = [];
  const released: ArrayBuffer[] = [];
  const unlistened: string[] = [];
  let ticket = 0;
  let lastFrame: { ticket: number; width: number; height: number } | null = null;

  const transport: Transport = {
    async ag<T>(name: string, args: Record<string, unknown> = {}) {
      calls.push([name, args]);
      if (options.refuse && name.startsWith('engine_')) throw options.refuse;
      const answer = (() => {
        switch (name) {
          case 'shell_ready':
            return PATH;
          case 'engine_sections':
            return fixtures.sections;
          case 'engine_tools':
            return fixtures.tools;
          case 'engine_open':
            return fixtures.opened;
          case 'engine_apply':
            return { ...fixtures.applied, ticket: ++ticket };
          case 'engine_view':
            return ++ticket;
          case 'engine_frame': {
            if (!lastFrame || lastFrame.ticket !== args.ticket) return null;
            const b = block(lastFrame.ticket, lastFrame.width, lastFrame.height);
            if (!options.shared) return b;
            for (const h of sharedHandlers) h(b, () => released.push(b));
            return 'shared';
          }
          case 'shell_folder':
            return options.folder ?? [args.path];
          case 'shell_pick':
            return PATH;
          default:
            throw new Error(`unknown Argentum command: ${name}`);
        }
      })();
      return answer as T;
    },
    async listen<T>(event: string, handler: (payload: T) => void) {
      const h = handler as (payload: unknown) => void;
      listeners.set(event, [...(listeners.get(event) ?? []), h]);
      return () => {
        unlistened.push(event);
        listeners.set(
          event,
          (listeners.get(event) ?? []).filter((x) => x !== h),
        );
      };
    },
    sharedBuffers(handler) {
      sharedHandlers.push(handler);
      return () => sharedHandlers.splice(sharedHandlers.indexOf(handler), 1);
    },
  };

  /** Send an event as the app does, and keep a frame's size for its block. */
  const emit = (event: string, payload: unknown) => {
    const e = payload as Event;
    if (event === ENGINE_EVENT && e.event === 'frame') lastFrame = e;
    for (const h of listeners.get(event) ?? []) h(payload);
  };
  const listening = (event: string) => listeners.get(event)?.length ?? 0;
  return { transport, calls, emit, released, unlistened, listening };
}

describe('the real client', () => {
  it('makes the calls the app reads, with the arguments it reads them with', async () => {
    const { transport, calls } = app();
    const connection = await connectApp(transport);
    const client = connection.client;
    const library = connection.app!.library;
    expect(connection.app!.first).toBe(PATH);
    expect(await client.sections()).toEqual(fixtures.sections);
    expect(await client.tools()).toEqual(fixtures.tools as ToolDescriptor[]);
    const opened = await client.open(PATH);
    expect(opened).toEqual(fixtures.opened);
    expect((await client.apply(opened.photoId, fixtures.patch)).edit).toEqual(fixtures.applied.edit);
    await client.view(opened.photoId, fixtures.view);
    expect(await client.frame(fixtures.events[0].ticket)).toBeNull();
    await library.folder(PATH);
    await library.pick(PATH);
    expect(Object.fromEntries(calls)).toEqual(wire.calls);
    expect(calls.map(([name]) => name)).toEqual(Object.keys(wire.calls));
  });

  it('reads a frame from the block the app sends', () => {
    const { ticket, width, height, bytes } = wire.frame;
    const buffer = new Uint8Array(bytes).buffer;
    const read = readFrame(buffer)!;
    expect(read.ticket).toBe(ticket);
    expect([read.pixels.width, read.pixels.height]).toEqual([width, height]);
    expect(Array.from(read.pixels.data)).toEqual(bytes.slice(12));
    expect(readFrame(new ArrayBuffer(0))).toBeNull();
    // Cut short: a broken answer, not a picture.
    expect(() => readFrame(buffer.slice(0, 20))).toThrow();
    expect(() => readFrame(buffer.slice(0, 6))).toThrow();
  });

  for (const shared of [false, true]) {
    it(`fetches a frame's pixels ${shared ? 'from a shared buffer, handed back at once' : 'as bytes'}`, async () => {
      const { transport, emit, released } = app({ shared });
      const client = createTauriEngine(transport);
      const ticket = await client.view(1, fixtures.view);
      emit(ENGINE_EVENT, { event: 'frame', ticket, photoId: 1, width: 4, height: 3 });
      const pixels = await client.frame(ticket);
      expect(pixels).toMatchObject({ width: 4, height: 3 });
      expect(pixels!.data).toHaveLength(4 * 3 * 4);
      expect(pixels!.data[0]).toBe(128);
      expect(released).toHaveLength(shared ? 1 : 0);
      // Replaced by a newer one: nothing.
      expect(await client.frame(ticket - 1)).toBeNull();
    });
  }

  it('rejects a refused call with the EngineError the app sends, as it is', async () => {
    for (const { patch, error } of fixtures.refused) {
      const client = createTauriEngine(app({ refuse: error }).transport);
      await expect(client.apply(1, patch as unknown as EditPatch)).rejects.toBe(error);
    }
    const client = createTauriEngine(app({ refuse: fixtures.refused[0].error }).transport);
    await expect(client.open(PATH)).rejects.toBe(fixtures.refused[0].error);
  });

  it('times each change to its pixels', async () => {
    const { transport, emit } = app({ shared: true });
    const client = createTauriEngine(transport);
    const ticks: number[] = [];
    client.onTickMs((ms) => ticks.push(ms));
    const { ticket } = await client.apply(1, fixtures.patch);
    emit(ENGINE_EVENT, { event: 'frame', ticket, photoId: 1, width: 2, height: 2 });
    await client.frame(ticket);
    expect(ticks).toHaveLength(1);
    expect(ticks[0]).toBeGreaterThanOrEqual(0);
  });

  it('answers opens in the order they were asked, whichever decodes first', async () => {
    const { transport } = app();
    const waiting: (() => void)[] = [];
    const slow: Transport = {
      ...transport,
      ag: <T>(name: string, args?: Record<string, unknown>) =>
        name === 'engine_open'
          ? new Promise<T>((resolve) => waiting.push(() => void transport.ag<T>(name, args).then(resolve)))
          : transport.ag<T>(name, args),
    };
    const client = createTauriEngine(slow);
    const order: string[] = [];
    const a = client.open('A.CR3').then(() => order.push('A'));
    const b = client.open('B.CR3').then(() => order.push('B'));
    await settle();
    // B is not even asked for until A is answered.
    expect(waiting).toHaveLength(1);
    waiting[0]();
    await settle();
    waiting[1]();
    await Promise.all([a, b]);
    expect(order).toEqual(['A', 'B']);
  });

  it('hands on the engine events in the shapes the engine sends them, until unsubscribed', async () => {
    const { transport, emit, unlistened } = app();
    const client = createTauriEngine(transport);
    const seen: Event[] = [];
    const stop = client.subscribe((e) => seen.push(e));
    await settle();
    for (const e of fixtures.events) emit(ENGINE_EVENT, e);
    emit('something-else', { event: 'frame' });
    expect(seen).toEqual(fixtures.events);
    stop();
    emit(ENGINE_EVENT, fixtures.events[0]);
    expect(seen).toHaveLength(fixtures.events.length);
    expect(unlistened).toEqual([ENGINE_EVENT]);
  });

  it('stops listening even when unsubscribed before Tauri said it was listening', async () => {
    const { transport, unlistened, listening } = app();
    const stop = createTauriEngine(transport).subscribe(() => undefined);
    stop();
    await settle();
    expect(unlistened).toEqual([ENGINE_EVENT]);
    expect(listening(ENGINE_EVENT)).toBe(0);
  });
});

describe('the shell on the real client', () => {
  it('opens the photo the app was started with, its folder as the filmstrip, and draws its frame', async () => {
    const folder = ['C:\\Photos\\2026-10-09\\IMG_0000.CR3', 'C:\\Photos\\2026-10-09\\IMG_0001.CR3'];
    const { transport, emit } = app({ folder, shared: true });
    const connection = await connectApp(transport);
    expect(connection.fake).toBe(false);
    expect(connection.app?.first).toBe(PATH);
    const store = createShellStore(connection.client, { decode: async () => null });
    await store.getState().init();
    store.getState().setViewSize({ width_px: 800, height_px: 600 });
    await showFolderOf(store, connection.app!.library, connection.app!.first!);
    await settle();
    const s = store.getState;
    expect(s().photos).toEqual(folder);
    // The folder's spelling of the path, so the filmstrip marks it.
    expect(s().path).toBe(folder[1]);
    expect(s().photo?.photoId).toBe(fixtures.opened.photoId);
    expect(s().sections).toEqual(fixtures.sections);

    const ticket = s().latestTicket;
    emit(ENGINE_EVENT, { event: 'frame', ticket, photoId: s().photo!.photoId, width: 40, height: 30 });
    await settle();
    expect(s().frame).toMatchObject({ ticket, size_px: { width_px: 40, height_px: 30 } });
  });
});
