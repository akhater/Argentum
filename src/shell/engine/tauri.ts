// The engine inside the app: the same EngineClient as the contract fake, over
// Argentum's one `ag` command (src-tauri/src/mods/dispatch.rs) and the
// ENGINE_EVENT event (src-tauri/src/mods/engine_bridge.rs). Nothing here knows
// a tool. Each answer is the engine's own, and a refused call rejects with
// the engine's EngineError, both passed through as the app sends them.
//
// A frame crosses as the engine's block: FRAME_HEADER_BYTES of ticket, width
// and height, then the RGBA rows (src-tauri/src/mods/engine_frame.rs). On
// Windows the block comes as a WebView2 shared buffer beside the answer,
// which is then FRAME_SHARED; elsewhere the answer is the block. `null` means a
// newer frame has replaced the one asked for. The pixels are a view of the
// block, never a copy: a shared buffer goes back to WebView2 once, when the
// pixels' `release` is called, after they are decoded or dropped.

import { listen } from '@tauri-apps/api/event';

import { ag } from '../../argentum/ag';
import {
  ENGINE_EVENT,
  FRAME_HEADER_BYTES,
  FRAME_SHARED,
  FRAME_SHARED_KEY,
  type Applied,
  type EngineError,
  type Event,
  type Opened,
  type RenderTicket,
} from '../contract';
import type { Connection, EngineClient, FramePixels, Library } from './client';

/** How long a frame announced as shared may take to arrive before it counts as lost. */
const SHARED_FRAME_TIMEOUT_MS = 5000;
/** What an open answers when a newer open replaced it before it began. */
export const REPLACED: EngineError = { kind: 'internal', message: 'replaced by a newer open' };

/**
 * A shared buffer, offered to each listener in turn. True takes it: the
 * listener then owns its `release`. One nobody takes goes back at once.
 */
type SharedBufferListener = (buffer: ArrayBuffer, release: () => void) => boolean;

/** How the client reaches the app: Tauri in the app, a recording in tests. */
export interface Transport {
  ag<T>(name: string, args?: Record<string, unknown>): Promise<T>;
  listen<T>(event: string, handler: (payload: T) => void): Promise<() => void>;
  /** WebView2 shared buffers the app posts. Returns the unsubscribe. */
  sharedBuffers(listener: SharedBufferListener): () => void;
}

/** WebView2's side of shared buffers, where there is one. */
export interface WebView2 {
  addEventListener(type: 'sharedbufferreceived', listener: (e: SharedBufferEvent) => void): void;
  removeEventListener(type: 'sharedbufferreceived', listener: (e: SharedBufferEvent) => void): void;
  releaseBuffer(buffer: ArrayBuffer): void;
}
export interface SharedBufferEvent {
  getBuffer(): ArrayBuffer;
  additionalData?: Record<string, unknown>;
}

/**
 * The page's one listener for WebView2's shared buffers. Every buffer is
 * handed back exactly once: by the listener that took it, or here.
 */
export function pageSharedBuffers(
  find: () => WebView2 | undefined = () => (window as { chrome?: { webview?: WebView2 } }).chrome?.webview,
): Transport['sharedBuffers'] {
  const listeners = new Set<SharedBufferListener>();
  let webview: WebView2 | undefined;
  const received = (e: SharedBufferEvent) => {
    if (e.additionalData?.[FRAME_SHARED_KEY] === undefined || !webview) return;
    const owner = webview;
    const buffer = e.getBuffer();
    let released = false;
    const release = () => {
      if (released) return;
      released = true;
      owner.releaseBuffer(buffer);
    };
    for (const listener of listeners) if (listener(buffer, release)) return;
    release();
  };
  return (listener) => {
    webview ??= find();
    if (!webview) return () => undefined;
    if (listeners.size === 0) webview.addEventListener('sharedbufferreceived', received);
    listeners.add(listener);
    return () => {
      listeners.delete(listener);
      if (listeners.size === 0) webview?.removeEventListener('sharedbufferreceived', received);
    };
  };
}

export const tauriTransport: Transport = {
  ag,
  listen: (event, handler) => listen(event, (e) => handler(e.payload as never)),
  sharedBuffers: pageSharedBuffers(),
};

/**
 * A frame's block as its ticket and pixels, a view of the block with
 * `release` to hand it back; null when there is none. Throws on a broken block.
 */
export function readFrame(
  block: ArrayBuffer,
  release?: () => void,
): { ticket: RenderTicket; pixels: FramePixels } | null {
  if (block.byteLength === 0) return null;
  if (block.byteLength < FRAME_HEADER_BYTES) throw new Error(`a frame of ${block.byteLength} bytes`);
  const head = new DataView(block, 0, FRAME_HEADER_BYTES);
  const [ticket, width, height] = [0, 4, 8].map((at) => head.getUint32(at, true));
  const pixelBytes = width * height * 4;
  if (block.byteLength !== FRAME_HEADER_BYTES + pixelBytes) {
    throw new Error(`a ${width} x ${height} frame of ${block.byteLength} bytes`);
  }
  const data = new Uint8ClampedArray(block, FRAME_HEADER_BYTES, pixelBytes);
  return { ticket, pixels: release ? { width, height, data, release } : { width, height, data } };
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
 * for it, which then owns its release. One nobody asked for, or older than
 * a frame already handed over, is not taken, and goes back at once.
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
      read = readFrame(buffer, release);
    } catch {
      return false;
    }
    if (!read) return false;
    for (const older of [...waiting.keys()].filter((t) => t < read.ticket)) settle(older, null);
    if (!waiting.has(read.ticket)) return false;
    settle(read.ticket, read.pixels);
    return true;
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
  // Opens run in the order they were asked, and one a newer open replaced
  // before its turn gives way without being sent.
  let opening: Promise<unknown> = Promise.resolve();
  let opensAsked = 0;

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
      const mine = ++opensAsked;
      const opened = opening.then(async () => {
        if (mine !== opensAsked) throw REPLACED;
        // null: the app gave it way to a newer open before decoding.
        const answer = await call<Opened | null>('engine_open', { path });
        if (!answer) throw REPLACED;
        return answer;
      });
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
      const answer = await call<ArrayBuffer | typeof FRAME_SHARED | null>('engine_frame', { ticket });
      if (answer === FRAME_SHARED) return delivered(ticket, await posted);
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

/** Show the window, which the app builds hidden. */
export const showWindow = (transport: Transport = tauriTransport) => transport.ag<null>('shell_show_window');

/** The real engine, and the photo the app was started with, if any. */
export async function connectApp(transport: Transport = tauriTransport): Promise<Connection> {
  await showWindow(transport);
  const first = await transport.ag<string | null>('shell_launch_photo');
  const client = createTauriEngine(transport);
  return {
    client,
    fake: false,
    photos: [],
    app: { first: first ?? undefined, library: appLibrary(transport), onTickMs: client.onTickMs },
  };
}
