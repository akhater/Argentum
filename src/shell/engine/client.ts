// The only way the shell reaches the engine. The calls mirror the engine's
// own (src-tauri/engine/src/engine.rs) and every type is the generated
// contract's, so the real engine and the contract fake are interchangeable.

import { isTauri } from '@tauri-apps/api/core';

import type { BrowseClient } from '../browse/client';
import type {
  Applied,
  EditPatch,
  Event,
  Opened,
  PhotoId,
  RenderTicket,
  SectionDescriptor,
  ToolDescriptor,
  View,
} from '../contract';

/** One rendered view: sRGB, 8 bits per channel, RGBA, row-major. */
export interface FramePixels {
  width: number;
  height: number;
  data: Uint8ClampedArray<ArrayBuffer>;
  /**
   * Hands the pixels' memory back, where it is borrowed (a WebView2 shared
   * buffer). Call it once the pixels are decoded or dropped; never read
   * `data` after.
   */
  release?(): void;
}

export interface EngineClient {
  /** The panels' sections, in the order they are shown. */
  sections(): Promise<SectionDescriptor[]>;
  tools(): Promise<ToolDescriptor[]>;
  open(path: string): Promise<Opened>;
  /**
   * The edit as the engine took it, and the render's ticket. Refused as a
   * whole, with an EngineError, for an unknown tool or setting or a value of
   * the wrong type. Every refused call rejects with an EngineError.
   */
  apply(photo: PhotoId, patch: EditPatch): Promise<Applied>;
  view(photo: PhotoId, view: View): Promise<RenderTicket>;
  /**
   * The pixels a `frame` event announced, or null once a newer frame has
   * replaced it. Not part of the contract types: inside the app they cross
   * as bytes (engine/tauri.ts).
   */
  frame(ticket: RenderTicket): Promise<FramePixels | null>;
  /** Frame, histogram and error events. Returns the unsubscribe. */
  subscribe(listener: (event: Event) => void): () => void;
}

/** Files on this computer, beyond browsing. Only inside the app. */
export interface Library {
  /** A file dialog for one RAW, in `from`'s folder; null when closed without one. */
  pick(from: string | null): Promise<string | null>;
}

/** What the window has inside the app, beside the engine. */
export interface AppConnection {
  /** The photo the app was started with. */
  first?: string;
  library: Library;
  /** Milliseconds from each change sent to its frame's pixels. Returns the unsubscribe. */
  onTickMs(listener: (ms: number) => void): () => void;
}

export interface Connection {
  client: EngineClient;
  /** True while the contract fake stands in for the engine. */
  fake: boolean;
  /** Folders and thumbnails: the app's, or the browse fake's made-up disk with the engine fake. */
  browse: BrowseClient;
  /** Inside the app only. */
  app?: AppConnection;
}

/**
 * The engine for this window: the real one inside the app, the contract fake
 * in a plain browser, or anywhere with `?engine=fake`.
 */
export async function connectEngine(): Promise<Connection> {
  const fake = new URLSearchParams(window.location.search).get('engine') === 'fake';
  if (isTauri() && !fake) {
    const [{ connectApp, showWindow }, { createTauriBrowse }] = await Promise.all([
      import('./tauri'),
      import('../browse/tauri'),
    ]);
    try {
      return { ...(await connectApp()), browse: createTauriBrowse() };
    } catch (e) {
      // The window is built hidden: show it, so the failure can be seen.
      await showWindow().catch(() => undefined);
      throw e;
    }
  }
  const [{ createFakeEngine }, { createWorkerPainter }, { createFakeBrowse, paintThumb }] = await Promise.all([
    import('./fake'),
    import('./fakePaint'),
    import('../browse/fake'),
  ]);
  return {
    client: createFakeEngine({ paint: createWorkerPainter() }),
    fake: true,
    browse: createFakeBrowse({ thumbMs: 25, paint: paintThumb }),
  };
}
