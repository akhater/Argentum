// The shell's one store, made of slices: the edit and its history
// (historySlice.ts), what is on screen (viewSlice.ts), and here what the
// engine offers, the photo open, and the panels' own state. Components read it
// through selectors (useShell in ShellContext.tsx).

import { createStore, type StoreApi } from 'zustand/vanilla';

import type { Event, Opened, SectionDescriptor, Section, ToolDescriptor } from './contract';
import type { EngineClient, FramePixels } from './engine/client';
import { createHistorySlice, type HistoryActions, type HistoryState } from './historySlice';
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
  error: string | null;
  /** The tab panel beside the photo; null when folded. */
  sideTab: SideTab | null;
  collapsed: Readonly<Partial<Record<Section, boolean>>>;
}

export interface CoreActions {
  /** Subscribe to the engine, read its sections and tools, and open the first photo. */
  init(): Promise<void>;
  dispose(): void;
  openPhoto(path: string): Promise<void>;
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
  /** Turns frame pixels into something drawable. Defaults to createImageBitmap where there is one. */
  decode?: (pixels: FramePixels) => Promise<ImageBitmap | null>;
}

const decodeToBitmap = (p: FramePixels) =>
  typeof createImageBitmap === 'function'
    ? createImageBitmap(new ImageData(new Uint8ClampedArray(p.data), p.width, p.height))
    : Promise.resolve(null);

export function createShellStore(client: EngineClient, options: StoreOptions = {}): ShellStore {
  // The subscription and the start-up, held so init and dispose can run twice (React's strict mode).
  let unsubscribe: (() => void) | null = null;
  let started: Promise<void> | null = null;

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
      try {
        const opened = await client.open(path);
        const s = get();
        s.resetHistory(opened.edit);
        s.resetView();
        set({ path, photo: { photoId: opened.photoId, width: opened.width, height: opened.height }, error: null });
        s.requestView();
      } catch (e) {
        get().fail(e);
      }
    },

    fail(error) {
      const message = typeof error === 'object' && error !== null && 'message' in error ? error.message : error;
      set({ error: String(message) });
    },

    receive(event) {
      const s = get();
      if (event.event === 'error') {
        set({ error: event.message });
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
