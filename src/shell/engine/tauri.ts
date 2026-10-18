// The engine inside the app: the same EngineClient as the contract fake, over
// Argentum's one `ag` command (src-tauri/src/mods/dispatch.rs) and the
// `ag-engine` event (src-tauri/src/mods/engine_bridge.rs). Nothing here knows
// a tool. Each answer is the engine's own, and a refused call rejects with
// the engine's EngineError, both passed through as the app sends them.
//
// A frame crosses as one block: a 12-byte header (ticket, width, height, each
// a little-endian u32), then the RGBA rows (src-tauri/src/mods/engine_frame.rs).
// On Windows the block comes as a WebView2 shared buffer beside the answer,
// which is then "shared"; elsewhere the answer is the block. `null` means a
// newer frame has replaced the one asked for.

import { listen } from '@tauri-apps/api/event';

import { ag } from '../../argentum/ag';
import type { Applied, Event, Opened, RenderTicket } from '../contract';
import type { Connection, EngineClient, FramePixels, Library } from './client';

/** The Tauri event engine events arrive as. */
export const ENGINE_EVENT = 'ag-engine';
/** Ticket, width, height. */
const HEADER_BYTES = 12;
/** How long a frame announced as shared may take to arrive before it counts as lost. */
const SHARED_FRAME_TIMEOUT_MS = 5000;

/** How the client reaches the app: Tauri in the app, a recording in tests. */
export interface Transport {
  ag<T>(name: string, args?: Record<string, unknown>): Promise<T>;
  listen<T>(event: string, handler: (payload: T) => void): Promise<() => void>;
  /** WebView2 shared buffers the app posts, each with how to hand it back. Returns the unsubscribe. */
  sharedBuffers(handler: (buffer: ArrayBuffer, release: () => void) => void): () => void;
}

/** WebView2's side of shared buffers, where there is one. */
interface WebView2 {
  addEventListener(type: 'sharedbufferreceived', listener: (e: SharedBufferEvent) => void): void;
  removeEventListener(type: 'sharedbufferreceived', listener: (e: SharedBufferEvent) => void): void;
  releaseBuffer(buffer: ArrayBuffer): void;
}
interface SharedBufferEvent {
  getBuffer(): ArrayBuffer;
  additionalData?: { agFrame?: number };
}

export const tauriTransport: Transport = {
  ag,
  listen: (event, handler) => listen(event, (e) => handler(e.payload as never)),
  sharedBuffers(handler) {
    const webview = (window as { chrome?: { webview?: WebView2 } }).chrome?.webview;
    if (!webview) return () => undefined;
    const received = (e: SharedBufferEvent) => {
      if (e.additionalData?.agFrame === undefined) return;
      const buffer = e.getBuffer();
      handler(buffer, () => webview.releaseBuffer(buffer));
    };
    webview.addEventListener('sharedbufferreceived', received);
    return () => webview.removeEventListener('sharedbufferreceived', received);
  },
};

/** A frame's block as its ticket and pixels; null when there is none. Throws on a broken block. */
export function readFrame(block: ArrayBuffer): { ticket: RenderTicket; pixels: FramePixels } | null {
  if (block.byteLength === 0) return null;
  if (block.byteLength < HEADER_BYTES) throw new Error(`a frame of ${block.byteLength} bytes`);
  const head = new DataView(block, 0, HEADER_BYTES);
  const [ticket, width, height] = [0, 4, 8].map((at) => head.getUint32(at, true));
  if (block.byteLength !== HEADER_BYTES + width * height * 4) {
    throw new Error(`a ${width} x ${height} frame of ${block.byteLength} bytes`);
  }
  return { ticket, pixels: { width, height, data: new Uint8ClampedArray(block, HEADER_BYTES) } };
}

/**
 * Listen until the returned function is called, including when it is called
 * before Tauri has said it is listening.
 */
function subscription<T>(transport: Transport, event: string, handler: (payload: T) => void): () => void {
  let gone = false;
  let stop: (() => void) | null = null;
  void transport
    .listen<T>(event, (payload) => {
      if (!gone) handler(payload);
    })
    .then((unlisten) => {
      if (gone) unlisten();
      else stop = unlisten;
    });
  return () => {
    gone = true;
    stop?.();
  };
}

/**
 * Frames posted as shared buffers, each handed to the `frame` call that asked
 * for it. A buffer is copied out and handed back at once, so nothing holds
 * WebView2's memory; one older than a frame already handed over is dropped.
 */
function sharedFrames(transport: Transport) {
  const waiting = new Map<
    RenderTicket,
    { resolve: (pixels: FramePixels | null) => void; timer: ReturnType<typeof setTimeout> }
  >();
  const settle = (ticket: RenderTicket, pixels: FramePixels | null) => {
    const w = waiting.get(ticket);
    if (!w) return;
    clearTimeout(w.timer);
    waiting.delete(ticket);
    w.resolve(pixels);
  };
  transport.sharedBuffers((buffer, release) => {
    let read: ReturnType<typeof readFrame>;
    try {
      read = readFrame(buffer.slice(0));
    } finally {
      release();
    }
    if (!read) return;
    for (const older of [...waiting.keys()].filter((t) => t < read.ticket)) settle(older, null);
    settle(read.ticket, read.pixels);
  });
  return {
    /** The pixels of `ticket`'s shared buffer, once it comes; null if it never does. */
    expect(ticket: RenderTicket): Promise<FramePixels | null> {
      settle(ticket, null);
      return new Promise((resolve) => {
        const timer = setTimeout(() => settle(ticket, null), SHARED_FRAME_TIMEOUT_MS);
        waiting.set(ticket, { resolve, timer });
      });
    },
    forget: (ticket: RenderTicket) => settle(ticket, null),
  };
}

/** The real client, and how long each change took to reach pixels. */
export function createTauriEngine(transport: Transport = tauriTransport): EngineClient & {
  /** Milliseconds from each change sent to its frame's pixels. Returns the unsubscribe. */
  onTickMs(listener: (ms: number) => void): () => void;
} {
  const call = transport.ag;
  const shared = sharedFrames(transport);
  const changedAt = new Map<RenderTicket, number>();
  const tickListeners = new Set<(ms: number) => void>();
  // Opens answer in the order they were asked: the photo asked for last is the one shown.
  let opening: Promise<unknown> = Promise.resolve();

  const delivered = (ticket: RenderTicket, pixels: FramePixels | null) => {
    const at = changedAt.get(ticket);
    for (const t of [...changedAt.keys()]) if (t <= ticket) changedAt.delete(t);
    if (pixels && at !== undefined) tickListeners.forEach((l) => l(performance.now() - at));
    return pixels;
  };

  return {
    sections: () => call('engine_sections'),
    tools: () => call('engine_tools'),
    open(path) {
      const opened = opening.then(() => call<Opened>('engine_open', { path }));
      opening = opened.catch(() => undefined);
      return opened;
    },
    async apply(photo, patch) {
      const at = performance.now();
      const applied = await call<Applied>('engine_apply', { photo, patch });
      changedAt.set(applied.ticket, at);
      return applied;
    },
    view: (photo, view) => call('engine_view', { photo, view }),
    async frame(ticket) {
      // Asked for before the call, as the buffer can come before the answer.
      const posted = shared.expect(ticket);
      const answer = await call<ArrayBuffer | 'shared' | null>('engine_frame', { ticket });
      if (answer === 'shared') return delivered(ticket, await posted);
      shared.forget(ticket);
      const read = answer ? readFrame(answer) : null;
      return delivered(ticket, read && read.ticket === ticket ? read.pixels : null);
    },
    subscribe: (listener) => subscription<Event>(transport, ENGINE_EVENT, listener),
    onTickMs(listener) {
      tickListeners.add(listener);
      return () => {
        tickListeners.delete(listener);
      };
    },
  };
}

/** The photos on this computer, through the app. */
export function appLibrary(transport: Transport = tauriTransport): Library {
  return {
    folder: (path) => transport.ag('shell_folder', { path }),
    pick: (from) => transport.ag('shell_pick', { from }),
  };
}

/**
 * The real engine, and the photo the app was started with, if any. The app
 * builds its window hidden; `shell_ready` shows it.
 */
export async function connectApp(transport: Transport = tauriTransport): Promise<Connection> {
  const first = await transport.ag<string | null>('shell_ready');
  const client = createTauriEngine(transport);
  return {
    client,
    fake: false,
    photos: [],
    app: { first: first ?? undefined, library: appLibrary(transport), onTickMs: client.onTickMs },
  };
}
