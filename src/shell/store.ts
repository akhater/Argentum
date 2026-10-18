// The shell's one store, made of slices: the edit and its history
// (historySlice.ts), what is on screen (viewSlice.ts), and here what the
// engine offers, the photo open, and the panels' own state. Components read it
// through selectors (useShell in ShellContext.tsx).

import { createStore, type StoreApi } from 'zustand/vanilla';

import type { EngineError, Event, Opened, SectionDescriptor, Section, ToolDescriptor } from './contract';
import type { EngineClient, FramePixels } from './engine/client';
import { createHistorySlice, type HistoryActions, type HistoryState } from './historySlice';
import { fileName } from './paths';
import { createViewSlice, type ViewActions, type ViewState } from './viewSlice';

export type SideTab = 'presets' | 'snapshots' | 'history';

export interface CoreState {
  /** The panels' sections, in the engine's order. */
  sections: readonly SectionDescriptor[];
  tools: readonly ToolDescriptor[];
  /** The filmstrip's photos. */
  photos: readonly string[];
  path: string | null;
  photo: Pick<Opened, 'photoId' | 'width' | 'height'> | null;
  /** The last refusal or failure, with its kind; anything unexpected is `internal`. */
  error: EngineError | null;
  /** The tab panel beside the photo; null when folded. */
  sideTab: SideTab | null;
  collapsed: Readonly<Partial<Record<Section, boolean>>>;
}

export interface CoreActions {
  /** Subscribe to the engine, read its sections and tools, and open the first photo. */
  init(): Promise<void>;
  dispose(): void;
  /** Open a photo. Of several asked for at once, the one asked for last is shown. */
  openPhoto(path: string): Promise<void>;
  /** Open a photo from disk, its folder becoming the filmstrip. */
  openFolder(path: string): Promise<void>;
  /** Show an error from a refused call: an EngineError, or anything thrown on the way. */
  fail(error: unknown): void;
  /** An engine event: errors are shown; frames and histograms of another photo are dropped. */
  receive(event: Event): void;
  setSideTab(tab: SideTab | null): void;
  toggleSection(section: Section): void;
}

export type Shell = CoreState & CoreActions & HistoryState & HistoryActions & ViewState & ViewActions;
export type ShellStore = StoreApi<Shell>;

/** A part of the store, given the whole store's set and get. */
export type Slice<T> = (set: ShellStore['setState'], get: ShellStore['getState']) => T;

export interface StoreOptions {
  photos?: readonly string[];
  /** The photos in a photo's folder, where there are files to list (inside the app). */
  folder?: (path: string) => Promise<readonly string[]>;
  /** Turns frame pixels into something drawable. Defaults to createImageBitmap where there is one. */
  decode?: (pixels: FramePixels) => Promise<ImageBitmap | null>;
}

/** What was thrown, as an EngineError: the engine's own, or anything else as `internal`. */
function asEngineError(error: unknown): EngineError {
  if (typeof error === 'object' && error !== null && 'kind' in error && 'message' in error) {
    return { kind: (error as EngineError).kind, message: String(error.message) };
  }
  const message = error instanceof Error ? error.message : String(error);
  return { kind: 'internal', message };
}

// Straight from the frame's pixels: no copy of the frame on this thread.
const decodeToBitmap = (p: FramePixels) =>
  typeof createImageBitmap === 'function'
    ? createImageBitmap(new ImageData(p.data, p.width, p.height))
    : Promise.resolve(null);

export function createShellStore(client: EngineClient, options: StoreOptions = {}): ShellStore {
  // The subscription and the start-up, held so init and dispose can run twice (React's strict mode).
  let unsubscribe: (() => void) | null = null;
  let started: Promise<void> | null = null;
  // Opens asked for so far: an answer to any but the newest is not shown.
  let opensAsked = 0;

  return createStore<Shell>()((set, get) => ({
    ...createHistorySlice(client)(set, get),
    ...createViewSlice(client, options.decode ?? decodeToBitmap)(set, get),

    sections: [],
    tools: [],
    photos: options.photos ?? [],
    path: null,
    photo: null,
    error: null,
    sideTab: 'history',
    collapsed: {},

    init() {
      unsubscribe ??= client.subscribe((event) => get().receive(event));
      started ??= (async () => {
        try {
          const [sections, tools] = await Promise.all([client.sections(), client.tools()]);
          set({ sections, tools });
          const first = get().photos[0];
          if (first) await get().openPhoto(first);
        } catch (e) {
          get().fail(e);
        }
      })();
      return started;
    },

    dispose() {
      unsubscribe?.();
      unsubscribe = null;
    },

    async openPhoto(path) {
      const mine = ++opensAsked;
      try {
        const opened = await client.open(path);
        if (mine !== opensAsked) return;
        const s = get();
        s.resetHistory(opened.edit);
        s.resetView();
        set({ path, photo: { photoId: opened.photoId, width: opened.width, height: opened.height }, error: null });
        s.requestView();
      } catch (e) {
        if (mine === opensAsked) get().fail(e);
      }
    },

    async openFolder(path) {
      let photos: readonly string[] = [];
      try {
        photos = options.folder ? await options.folder(path) : [];
      } catch (e) {
        get().fail(e);
      }
      // The folder's own spelling of the path, so the filmstrip marks it.
      const same = photos.find((p) => fileName(p) === fileName(path)) ?? path;
      set({ photos: photos.includes(same) ? photos : [same, ...photos] });
      await get().openPhoto(same);
    },

    fail(error) {
      set({ error: asEngineError(error) });
    },

    receive(event) {
      const s = get();
      if (event.event === 'error') {
        set({ error: { kind: event.kind, message: event.message } });
        return;
      }
      if (!s.photo || event.photoId !== s.photo.photoId) return;
      if (event.event === 'histogram') s.takeHistogram(event.ticket, event.bins);
      else void s.takeFrame(event.ticket);
    },

    setSideTab: (sideTab) => set({ sideTab }),
    toggleSection: (section) => set({ collapsed: { ...get().collapsed, [section]: !get().collapsed[section] } }),
  }));
}
