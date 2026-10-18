// The only way the shell reaches the engine. The calls mirror the engine's
// own (src-tauri/engine/src/engine.rs) and every type is the generated
// contract's, so the real engine and the contract fake are interchangeable.

import { isTauri } from '@tauri-apps/api/core';

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
  data: Uint8ClampedArray;
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

/** Photos on this computer. Only inside the app: a plain browser has the fake's samples. */
export interface Library {
  /** The RAWs in a photo's folder, sorted by name. */
  folder(path: string): Promise<string[]>;
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
  /** What the filmstrip shows until a photo is opened from a folder. */
  photos: string[];
  /** Inside the app only. */
  app?: AppConnection;
}

/**
 * The engine for this window: the real one inside the app, the contract fake
 * in a plain browser, or anywhere with `?engine=fake`.
 */
export async function connectEngine(): Promise<Connection> {
  const fake = new URLSearchParams(window.location.search).get('engine') === 'fake';
  if (isTauri() && !fake) return (await import('./tauri')).connectApp();
  const [{ createFakeEngine, FAKE_PHOTOS }, { createWorkerPainter }] = await Promise.all([
    import('./fake'),
    import('./fakePaint'),
  ]);
  return { client: createFakeEngine({ paint: createWorkerPainter() }), fake: true, photos: FAKE_PHOTOS };
}
