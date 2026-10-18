// The real client, as it runs inside the app, against the engine's contract
// fixtures: its calls with the arguments the app's dispatcher reads
// (src-tauri/src/mods/dispatch.rs, whose test reads the same shapes), and
// the engine's own frame block.

import { describe, expect, it } from 'vitest';

import fixtures from '../contract.fixtures.json';
import { ENGINE_EVENT, FRAME_SHARED_KEY, type EditPatch, type Event, type ToolDescriptor } from '../contract';
import {
  connectApp,
  createTauriEngine,
  pageSharedBuffers,
  readFrame,
  REPLACED,
  type SharedBufferEvent,
  type Transport,
  type WebView2,
} from '../engine/tauri';
import { createShellStore } from '../store';
import { settle } from './helpers';

const PATH = 'C:/Photos/IMG_0001.CR3';

/** A frame event the app sends, for a frame of `width` x `height`. */
const frameEvent = (ticket: number, photoId: number, width: number, height: number): Event => ({
  event: 'frame',
  ticket,
  photoId,
  placement: { xPx: 0, yPx: 0, widthPx: width, heightPx: height },
});

/** A frame's block: the header, then grey pixels. */
function block(ticket: number, width: number, height: number): ArrayBuffer {
  const bytes = new Uint8Array(12 + width * height * 4).fill(128);
  const head = new DataView(bytes.buffer);
  [ticket, width, height].forEach((v, i) => head.setUint32(i * 4, v, true));
  return bytes.buffer;
}

/**
 * WebView2's side of shared buffers, as strict as the real one: handing a
 * buffer back twice throws, as `releaseBuffer` does on a detached buffer.
 */
function webView2() {
  const listeners = new Set<(e: SharedBufferEvent) => void>();
  const released = new Set<ArrayBuffer>();
  const webview: WebView2 = {
    addEventListener: (_type, l) => listeners.add(l),
    removeEventListener: (_type, l) => listeners.delete(l),
    releaseBuffer(buffer) {
      if (released.has(buffer)) throw new TypeError('Could not transfer ArrayBuffer.');
      released.add(buffer);
    },
  };
  const post = (buffer: ArrayBuffer, ticket: number) =>
    listeners.forEach((l) => l({ getBuffer: () => buffer, additionalData: { [FRAME_SHARED_KEY]: ticket } }));
  return { webview, post, released, listening: () => listeners.size };
}

/**
 * The app as the client sees it: answers from the fixtures, every call
 * recorded. With `shared`, frames come as shared buffers, as on Windows,
 * through the page's real shared-buffer listener.
 */
function app(options: { folder?: string[]; shared?: boolean; refuse?: unknown } = {}) {
  const calls: [string, Record<string, unknown>][] = [];
  const listeners = new Map<string, ((payload: unknown) => void)[]>();
  const unlistened: string[] = [];
  const page = webView2();
  let ticket = 0;
  let lastFrame: { ticket: number; width: number; height: number } | null = null;

  const transport: Transport = {
    async ag<T>(name: string, args: Record<string, unknown> = {}) {
      calls.push([name, args]);
      if (options.refuse && name.startsWith('engine_')) throw options.refuse;
      const answer = (() => {
        switch (name) {
          case 'shell_show_window':
            return null;
          case 'shell_launch_photo':
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
            // The buffer can arrive before the answer.
            page.post(b, lastFrame.ticket);
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
    sharedBuffers: pageSharedBuffers(() => page.webview),
  };

  /** Send an event as the app does, and keep a frame's size for its block. */
  const emit = (event: string, payload: unknown) => {
    const e = payload as Event;
    if (event === ENGINE_EVENT && e.event === 'frame') {
      lastFrame = { ticket: e.ticket, width: e.placement.widthPx, height: e.placement.heightPx };
    }
    for (const h of listeners.get(event) ?? []) h(payload);
  };
  const listening = (event: string) => listeners.get(event)?.length ?? 0;
  return { transport, calls, emit, page, unlistened, listening };
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
    const photo = fixtures.opened.photoId;
    expect(calls).toEqual([
      ['shell_show_window', {}],
      ['shell_launch_photo', {}],
      ['engine_sections', {}],
      ['engine_tools', {}],
      ['engine_open', { path: PATH }],
      ['engine_apply', { photo, patch: fixtures.patch }],
      ['engine_view', { photo, view: fixtures.view }],
      ['engine_frame', { ticket: fixtures.events[0].ticket }],
      ['shell_folder', { path: PATH }],
      ['shell_pick', { from: PATH }],
    ]);
  });

  it('reads a frame from the block the app sends, as a view of it', () => {
    const { ticket, widthPx: width, heightPx: height, rgba, bytes } = fixtures.frameBlock;
    const buffer = new Uint8Array(bytes).buffer;
    const read = readFrame(buffer)!;
    expect(read.ticket).toBe(ticket);
    expect([read.pixels.width, read.pixels.height]).toEqual([width, height]);
    expect(Array.from(read.pixels.data)).toEqual(rgba);
    expect(read.pixels.data.buffer).toBe(buffer);
    expect(readFrame(new ArrayBuffer(0))).toBeNull();
    // Cut short: a broken answer, not a picture.
    expect(() => readFrame(buffer.slice(0, bytes.length - 1))).toThrow();
    expect(() => readFrame(buffer.slice(0, 6))).toThrow();
  });

  for (const shared of [false, true]) {
    it(`fetches a frame's pixels ${shared ? 'on a shared buffer, handed back once on release' : 'as bytes'}`, async () => {
      const { transport, emit, page } = app({ shared });
      const client = createTauriEngine(transport);
      const ticket = await client.view(1, fixtures.view);
      emit(ENGINE_EVENT, frameEvent(ticket, 1, 4, 3));
      const pixels = (await client.frame(ticket))!;
      expect(pixels).toMatchObject({ width: 4, height: 3 });
      expect(pixels.data).toHaveLength(4 * 3 * 4);
      expect(pixels.data[0]).toBe(128);
      // Not handed back until the pixels are done with, then exactly once.
      expect(page.released.size).toBe(0);
      pixels.release?.();
      pixels.release?.();
      expect(page.released.size).toBe(shared ? 1 : 0);
      // Replaced by a newer one: nothing.
      expect(await client.frame(ticket - 1)).toBeNull();
    });
  }

  it('hands back at once a shared buffer nobody asked for, whatever the clients on the page', () => {
    const { transport, page } = app({ shared: true });
    // Two clients, as React's strict mode makes in development.
    createTauriEngine(transport);
    createTauriEngine(transport);
    expect(page.listening()).toBe(1);
    const stray = block(9, 2, 2);
    page.post(stray, 9);
    expect(page.released).toEqual(new Set([stray]));
    // A broken block is handed back too.
    const broken = new ArrayBuffer(5);
    page.post(broken, 10);
    expect(page.released.has(broken)).toBe(true);
  });

  it('gives a shared buffer to the client that asked for it, and only that one', async () => {
    const { transport, emit, page } = app({ shared: true });
    const idle = createTauriEngine(transport);
    const asking = createTauriEngine(transport);
    void idle;
    const ticket = await asking.view(1, fixtures.view);
    emit(ENGINE_EVENT, frameEvent(ticket, 1, 2, 2));
    const pixels = (await asking.frame(ticket))!;
    expect(page.released.size).toBe(0);
    pixels.release?.();
    expect(page.released.size).toBe(1);
  });

  it('the store hands a shared frame back once it is decoded, and builds no copy of it', async () => {
    const { transport, emit, page } = app({ shared: true });
    const client = createTauriEngine(transport);
    const decoded: Uint8ClampedArray[] = [];
    const store = createShellStore(client, {
      photos: [PATH],
      decode: async (p) => {
        decoded.push(p.data);
        expect(page.released.size).toBe(0);
        return null;
      },
    });
    await store.getState().init();
    store.getState().setViewSize({ width_px: 800, height_px: 600 });
    await settle();
    const ticket = store.getState().latestTicket;
    emit(ENGINE_EVENT, frameEvent(ticket, fixtures.opened.photoId, 4, 4));
    await settle();
    expect(decoded).toHaveLength(1);
    expect(page.released.size).toBe(1);
    expect(page.released.has(decoded[0].buffer as ArrayBuffer)).toBe(true);
  });

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
    emit(ENGINE_EVENT, frameEvent(ticket, 1, 2, 2));
    (await client.frame(ticket))?.release?.();
    expect(ticks).toHaveLength(1);
    expect(ticks[0]).toBeGreaterThanOrEqual(0);
  });

  it('opens in the order asked, and skips one a newer open replaced before its turn', async () => {
    const { transport, calls } = app();
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
    await settle();
    // A is under way; B and C wait their turn behind it.
    const b = client.open('B.CR3').then(
      () => order.push('B'),
      (e) => order.push(e === REPLACED ? 'B replaced' : 'B failed'),
    );
    const c = client.open('C.CR3').then(() => order.push('C'));
    await settle();
    expect(waiting).toHaveLength(1);
    waiting[0]();
    await settle();
    // B was replaced by C before its turn, and never sent.
    expect(waiting).toHaveLength(2);
    waiting[1]();
    await Promise.all([a, b, c]);
    expect(order).toEqual(['A', 'B replaced', 'C']);
    expect(calls.filter(([n]) => n === 'engine_open').map(([, x]) => x.path)).toEqual(['A.CR3', 'C.CR3']);
  });

  it('takes an open the app gave way to as replaced', async () => {
    const { transport } = app();
    const gaveWay: Transport = {
      ...transport,
      ag: <T>(name: string, args?: Record<string, unknown>) =>
        name === 'engine_open' ? Promise.resolve(null as T) : transport.ag<T>(name, args),
    };
    await expect(createTauriEngine(gaveWay).open(PATH)).rejects.toBe(REPLACED);
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
    const store = createShellStore(connection.client, {
      folder: connection.app!.library.folder,
      decode: async () => null,
    });
    await store.getState().init();
    store.getState().setViewSize({ width_px: 800, height_px: 600 });
    await store.getState().openFolder(connection.app!.first!);
    await settle();
    const s = store.getState;
    expect(s().photos).toEqual(folder);
    // The folder's spelling of the path, so the filmstrip marks it.
    expect(s().path).toBe(folder[1]);
    expect(s().photo?.photoId).toBe(fixtures.opened.photoId);
    expect(s().sections).toEqual(fixtures.sections);

    const ticket = s().latestTicket;
    emit(ENGINE_EVENT, frameEvent(ticket, s().photo!.photoId, 40, 30));
    await settle();
    expect(s().frame).toMatchObject({ ticket, placement: { xPx: 0, yPx: 0, widthPx: 40, heightPx: 30 } });
  });

  it('shows the photo opened last, and no error for one replaced on the way', async () => {
    const { transport } = app();
    const waiting = new Map<string, () => void>();
    const slow: Transport = {
      ...transport,
      ag: <T>(name: string, args?: Record<string, unknown>) =>
        name === 'engine_open'
          ? new Promise<T>((resolve) =>
              waiting.set(String(args?.path), () => void transport.ag<T>(name, args).then(resolve)),
            )
          : transport.ag<T>(name, args),
    };
    const store = createShellStore(createTauriEngine(slow), { decode: async () => null });
    await store.getState().init();
    const a = store.getState().openPhoto('A.CR3');
    await settle();
    const b = store.getState().openPhoto('B.CR3');
    const c = store.getState().openPhoto('C.CR3');
    await settle();
    expect([...waiting.keys()]).toEqual(['A.CR3']);
    waiting.get('A.CR3')!();
    await settle();
    waiting.get('C.CR3')!();
    await Promise.all([a, b, c]);
    expect(store.getState().path).toBe('C.CR3');
    expect(store.getState().error).toBeNull();
  });
});
