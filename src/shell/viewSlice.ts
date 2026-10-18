// What is on screen, as one slice of the shell's store: the zoom and centre,
// the renders asked for by ticket, and the latest frame and histogram. Renders
// are latest-wins: anything for a ticket older than one already seen is
// dropped, and so are pixels that arrive after a newer frame.

import type { Placement, RenderTicket, View } from './contract';
import type { EngineClient, FramePixels } from './engine/client';
import type { Shell, Slice } from './store';
import { PHOTO_CENTER, viewFor, type PhotoPoint, type SizePx, type Zoom } from './view';

/** A render asked for: whether it is of the whole photo fitted, which the Navigator shows. */
export interface RenderRequest {
  readonly fit: boolean;
}

/** A frame ready to draw. `image` is null where there is nothing to decode into (tests). */
export interface ShownFrame {
  readonly ticket: RenderTicket;
  /** Where it sits in its view, in device pixels, as the engine placed it. */
  readonly placement: Placement;
  readonly image: ImageBitmap | null;
  /** What it is a render of. */
  readonly request: RenderRequest;
}

export interface ViewState {
  zoom: Zoom;
  center: PhotoPoint;
  /** The photo area's size in screen pixels. */
  viewSize_px: SizePx | null;
  /** The newest ticket seen. A frame or histogram for an older one is stale. */
  latestTicket: RenderTicket;
  /** The newest render asked for: what a frame shows until its own ticket is known. */
  requested: RenderRequest | null;
  /** Renders asked for and not yet shown, by ticket. */
  requests: ReadonlyMap<RenderTicket, RenderRequest>;
  frame: ShownFrame | null;
  /** The last frame of the whole photo, for the Navigator and the filmstrip. */
  fitFrame: ShownFrame | null;
  /** The engine's luminance bins. */
  histogram: readonly number[] | null;
}

export interface ViewActions {
  /** Forget the last photo's view and frames. */
  resetView(): void;
  setViewSize(size: SizePx): void;
  setZoom(zoom: Zoom, center?: PhotoPoint): void;
  setCenter(center: PhotoPoint): void;
  /** Ask the engine to render the current view of the photo shown. */
  requestView(): void;
  /** A ticket the engine handed out, for `request`. */
  recordTicket(ticket: RenderTicket, request: RenderRequest | null): void;
  /** A frame of the photo shown was announced: fetch, decode and show it, unless a newer one came first. */
  takeFrame(ticket: RenderTicket, placement: Placement): Promise<void>;
  takeHistogram(ticket: RenderTicket, bins: readonly number[]): void;
}

/** The view to render now, or null while there is no photo or nowhere to draw. */
export const currentView = (s: Pick<Shell, 'photo' | 'viewSize_px' | 'zoom' | 'center'>): View | null =>
  s.photo && s.viewSize_px ? viewFor(s.photo, s.viewSize_px, s.zoom, s.center) : null;

const release = (old: ShownFrame | null, keep: readonly (ShownFrame | null)[]) => {
  if (old?.image && !keep.some((k) => k?.image === old.image)) old.image.close();
};

export const createViewSlice =
  (
    client: EngineClient,
    decode: (pixels: FramePixels) => Promise<ImageBitmap | null>,
  ): Slice<ViewState & ViewActions> =>
  (set, get) => ({
    zoom: 'fit',
    center: PHOTO_CENTER,
    viewSize_px: null,
    latestTicket: 0,
    requested: null,
    requests: new Map(),
    frame: null,
    fitFrame: null,
    histogram: null,

    resetView() {
      const s = get();
      release(s.frame, []);
      release(s.fitFrame, []);
      set({ frame: null, fitFrame: null, histogram: null, zoom: 'fit', center: PHOTO_CENTER, requests: new Map() });
    },

    setViewSize(size) {
      const v = get().viewSize_px;
      if (v && v.width_px === size.width_px && v.height_px === size.height_px) return;
      set({ viewSize_px: size });
      get().requestView();
    },

    setZoom(zoom, center) {
      set({ zoom, center: zoom === 'fit' ? PHOTO_CENTER : (center ?? get().center) });
      get().requestView();
    },

    setCenter(center) {
      const view = currentView({ ...get(), center });
      if (!view) return;
      set({ center: { x: view.centerX, y: view.centerY } });
      get().requestView();
    },

    requestView() {
      const s = get();
      const view = currentView(s);
      if (!s.photo || !view) return;
      const request: RenderRequest = { fit: s.zoom === 'fit' };
      set({ requested: request });
      client.view(s.photo.photoId, view).then(
        (ticket) => get().recordTicket(ticket, request),
        (e) => get().fail(e),
      );
    },

    recordTicket(ticket, request) {
      const s = get();
      // Answered after its frame arrived: nothing left to record.
      if (ticket < s.latestTicket) return;
      set({
        latestTicket: ticket,
        requests: request ? new Map(s.requests).set(ticket, request) : s.requests,
      });
    },

    async takeFrame(ticket, placement) {
      if (ticket < get().latestTicket) return;
      set({ latestTicket: ticket });
      const pixels = await client.frame(ticket);
      if (!pixels) return;
      let image: ImageBitmap | null;
      try {
        if (get().latestTicket !== ticket) return;
        image = await decode(pixels);
      } finally {
        pixels.release?.();
      }
      const s = get();
      const request = s.requests.get(ticket) ?? s.requested;
      if (s.latestTicket !== ticket || !request) {
        image?.close();
        return;
      }
      const shown: ShownFrame = { ticket, placement, image, request };
      const fitFrame = request.fit ? shown : s.fitFrame;
      // Requests up to this one are done.
      const requests = new Map([...s.requests].filter(([t]) => t > ticket));
      set({ frame: shown, fitFrame, requests });
      release(s.frame, [shown, fitFrame]);
      if (request.fit) release(s.fitFrame, [shown]);
    },

    takeHistogram(ticket, bins) {
      if (ticket < get().latestTicket) return;
      set({ latestTicket: ticket, histogram: bins });
    },
  });
