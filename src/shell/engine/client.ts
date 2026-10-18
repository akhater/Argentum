// The only way the shell reaches the engine. The calls mirror the engine's
// own (src-tauri/engine/src/engine.rs) and every type is the generated
// contract's, so the real engine and the contract fake are interchangeable.

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
   * replaced it. Not part of the contract types: how pixels cross is decided
   * when the engine is wired in.
   */
  frame(ticket: RenderTicket): Promise<FramePixels | null>;
  /** Frame, histogram and error events. Returns the unsubscribe. */
  subscribe(listener: (event: Event) => void): () => void;
}

export interface Connection {
  client: EngineClient;
  /** True while the contract fake stands in for the engine. */
  fake: boolean;
  /** What the filmstrip shows until a library hands the editor its photos. */
  photos: string[];
}

/**
 * The engine for this window. It is not wired into the app yet (DEC-29), so
 * for now this is always the contract fake. Once it is, the real client goes
 * here and `?engine=fake` keeps the fake reachable for development.
 */
export async function connectEngine(): Promise<Connection> {
  const [{ createFakeEngine, FAKE_PHOTOS }, { createWorkerPainter }] = await Promise.all([
    import('./fake'),
    import('./fakePaint'),
  ]);
  return { client: createFakeEngine({ paint: createWorkerPainter() }), fake: true, photos: FAKE_PHOTOS };
}
