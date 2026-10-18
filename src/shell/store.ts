// The shell's one store. Components read it through selectors (useShell in
// ShellContext.tsx). Every change to an edit is a patch sent to the engine,
// with its inverse kept for undo; the document is never snapshotted for undo.
//
// Changes show at once and are confirmed by the engine afterwards. The engine
// refuses a bad patch as a whole, so the store keeps the edit the engine has
// accepted and the patches still on their way, and a refusal rebuilds the
// edit from those: the refused change is rolled back and nothing else is.

import { createStore, type StoreApi } from 'zustand/vanilla';

import type {
  EditDocument,
  EditPatch,
  Event,
  PhotoId,
  RenderTicket,
  SectionDescriptor,
  SettingValue,
  ToolDescriptor,
  View,
} from './contract';
import type { EngineClient, FramePixels } from './engine/client';
import {
  applyPatch,
  changesFromDefaults,
  composePatches,
  diffDocuments,
  emptyDocument,
  inverseOf,
  isEmptyPatch,
  resetPatch,
  valueOf,
} from './edit';

export type HistoryLabel =
  | { kind: 'set'; toolId: string; key: string; value: SettingValue }
  | { kind: 'resetTool'; toolId: string }
  | { kind: 'resetSection'; section: string }
  | { kind: 'resetAll' }
  | { kind: 'preset'; name: string }
  | { kind: 'snapshot'; name: string };

export interface HistoryEntry {
  id: number;
  label: HistoryLabel;
  patch: EditPatch;
  inverse: EditPatch;
  /** A slider drag still under way: its further ticks fold into this entry. */
  open: boolean;
}

export type Mode = 'edit' | 'crop' | 'spot' | 'masking';
export type SideTab = 'presets' | 'snapshots' | 'history';
export type Zoom = 'fit' | number;

/** A frame ready to draw. `image` is null where there is nothing to decode into (tests). */
export interface ShownFrame {
  ticket: RenderTicket;
  width: number;
  height: number;
  image: ImageBitmap | null;
  /** The view it was rendered for. */
  view: View | null;
}

export interface ShellState {
  /** The panels' sections, in the engine's order. */
  sections: SectionDescriptor[];
  tools: ToolDescriptor[];
  photos: string[];
  path: string | null;
  photo: { id: PhotoId; width: number; height: number } | null;
  doc: EditDocument;
  /** Steps applied, oldest first. */
  past: HistoryEntry[];
  /** Steps undone, the next to redo last. */
  future: HistoryEntry[];
  /** The newest ticket seen. A frame or histogram for an older one is stale. */
  latestTicket: RenderTicket;
  frame: ShownFrame | null;
  /** The last frame of the whole photo, for the Navigator. */
  fitFrame: ShownFrame | null;
  /** 256 luminance bins. */
  histogram: number[] | null;
  zoom: Zoom;
  /** The photo point at the view's centre, 0..1. */
  center: { x: number; y: number };
  /** The photo area's size in screen pixels. */
  viewSize: { width: number; height: number } | null;
  error: string | null;
  mode: Mode;
  /** The tab panel beside the photo; null when folded. */
  sideTab: SideTab | null;
  collapsed: Record<string, boolean>;
  snapshots: { name: string; doc: EditDocument }[];
  presets: { name: string; patch: EditPatch }[];
}

export interface ShellActions {
  init(): Promise<void>;
  dispose(): void;
  openPhoto(path: string): Promise<void>;
  /** One setting changed. With `gesture`, the ticks of one drag are one undo step. */
  setSetting(toolId: string, key: string, value: SettingValue, options?: { gesture?: boolean }): void;
  /** The drag that `setSetting(..., { gesture: true })` began is over. */
  endGesture(): void;
  commit(patch: EditPatch, label: HistoryLabel): void;
  resetTool(toolId: string): void;
  resetSection(section: string): void;
  undo(): void;
  redo(): void;
  /** Undo or redo until `length` steps are applied. */
  travel(length: number): void;
  setViewSize(width: number, height: number): void;
  setZoom(zoom: Zoom, center?: { x: number; y: number }): void;
  setCenter(x: number, y: number): void;
  setMode(mode: Mode): void;
  setSideTab(tab: SideTab | null): void;
  toggleSection(section: string): void;
  addSnapshot(name: string): void;
  applySnapshot(index: number): void;
  savePreset(name: string): void;
  applyPreset(index: number): void;
  /** What the store does with an engine event. */
  receive(event: Event): void;
}

export type Shell = ShellState & ShellActions;
export type ShellStore = StoreApi<Shell>;

export interface StoreOptions {
  photos?: string[];
  /** Turns frame pixels into something drawable. Defaults to createImageBitmap where there is one. */
  decode?: (pixels: FramePixels) => Promise<ImageBitmap | null>;
}

const defaultDecode = (p: FramePixels) =>
  typeof createImageBitmap === 'function'
    ? createImageBitmap(new ImageData(new Uint8ClampedArray(p.data), p.width, p.height))
    : Promise.resolve(null);

/** The zoom at which the whole photo fits the view. */
export function fitZoom(s: Pick<ShellState, 'photo' | 'viewSize'>): number {
  if (!s.photo || !s.viewSize) return 1;
  return Math.min(s.viewSize.width / s.photo.width, s.viewSize.height / s.photo.height);
}

/** What the engine is asked to draw, or null while there is nothing to draw into. */
export function viewOf(s: Pick<ShellState, 'photo' | 'viewSize' | 'zoom' | 'center'>): View | null {
  if (!s.photo || !s.viewSize || s.viewSize.width < 1 || s.viewSize.height < 1) return null;
  const zoom = s.zoom === 'fit' ? fitZoom(s) : s.zoom;
  const { width, height } = s.viewSize;
  // Keep the photo over the view along each axis it overflows, centred along the others.
  const axis = (c: number, view: number, photo: number) => {
    const half = view / (2 * photo * zoom);
    return half >= 0.5 ? 0.5 : Math.min(1 - half, Math.max(half, c));
  };
  return {
    zoom,
    centerX: axis(s.center.x, width, s.photo.width),
    centerY: axis(s.center.y, height, s.photo.height),
    width,
    height,
  };
}

export function createShellStore(client: EngineClient, options: StoreOptions = {}): ShellStore {
  const decode = options.decode ?? defaultDecode;
  let unsubscribe: (() => void) | null = null;
  let started: Promise<void> | null = null;
  // Whether the newest request was for the whole photo, so its frame can feed the Navigator.
  let latestIsFit = true;
  // The newest view asked for: what every frame from now on is a picture of.
  let lastView: View | null = null;
  // The edit as the engine has accepted it, and the patches it has not answered yet.
  let confirmed = emptyDocument();
  let pending: { seq: number; patch: EditPatch; entry?: number }[] = [];
  let seq = 0;
  let entryId = 0;

  return createStore<Shell>()((set, get) => {
    const noteTicket = (ticket: RenderTicket) => {
      if (ticket > get().latestTicket) set({ latestTicket: ticket });
    };
    const failed = (e: unknown) => set({ error: e instanceof Error ? e.message : String(e) });

    const send = (patch: EditPatch, entry?: number) => {
      const { photo, tools } = get();
      if (!photo || isEmptyPatch(patch)) return;
      const mine = { seq: ++seq, patch, entry };
      pending.push(mine);
      latestIsFit = get().zoom === 'fit';
      client.apply(photo.id, patch).then(
        (ticket) => {
          if (get().photo?.id !== photo.id) return;
          confirmed = applyPatch(confirmed, patch, tools);
          pending = pending.filter((p) => p !== mine);
          noteTicket(ticket);
        },
        (e) => {
          if (get().photo?.id !== photo.id) return;
          pending = pending.filter((p) => p !== mine);
          const s = get();
          const keep = (h: HistoryEntry) => entry === undefined || h.id !== entry;
          set({
            doc: pending.reduce((doc, p) => applyPatch(doc, p.patch, s.tools), confirmed),
            past: s.past.filter(keep),
            future: s.future.filter(keep),
          });
          failed(e);
        },
      );
    };

    const requestView = () => {
      const s = get();
      const view = viewOf(s);
      if (!s.photo || !view) return;
      latestIsFit = s.zoom === 'fit';
      lastView = view;
      client.view(s.photo.id, view).then(noteTicket, failed);
    };

    /** Close a drag left open, so the next change starts a step of its own. */
    const closeOpen = (past: HistoryEntry[]) =>
      past.length && past[past.length - 1].open
        ? [...past.slice(0, -1), { ...past[past.length - 1], open: false }]
        : past;

    const release = (old: ShownFrame | null, keep: (ShownFrame | null)[]) => {
      if (old?.image && !keep.some((k) => k?.image === old.image)) old.image.close();
    };

    const showFrame = async (ticket: RenderTicket, fit: boolean) => {
      const pixels = await client.frame(ticket);
      if (!pixels || get().latestTicket !== ticket) return;
      const image = await decode(pixels);
      const s = get();
      if (s.latestTicket !== ticket) {
        image?.close();
        return;
      }
      const shown: ShownFrame = { ticket, width: pixels.width, height: pixels.height, image, view: lastView };
      const fitFrame = fit ? shown : s.fitFrame;
      set({ frame: shown, fitFrame });
      release(s.frame, [shown, fitFrame]);
      if (fit) release(s.fitFrame, [shown]);
    };

    return {
      sections: [],
      tools: [],
      photos: options.photos ?? [],
      path: null,
      photo: null,
      doc: emptyDocument(),
      past: [],
      future: [],
      latestTicket: 0,
      frame: null,
      fitFrame: null,
      histogram: null,
      zoom: 'fit',
      center: { x: 0.5, y: 0.5 },
      viewSize: null,
      error: null,
      mode: 'edit',
      sideTab: 'history',
      collapsed: {},
      snapshots: [],
      presets: [],

      init() {
        unsubscribe ??= client.subscribe((event) => get().receive(event));
        started ??= (async () => {
          try {
            const [sections, tools] = await Promise.all([client.sections(), client.tools()]);
            set({ sections, tools });
            const first = get().photos[0];
            if (first) await get().openPhoto(first);
          } catch (e) {
            failed(e);
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
          confirmed = opened.edit;
          pending = [];
          release(s.frame, []);
          release(s.fitFrame, []);
          set({
            path,
            photo: { id: opened.photoId, width: opened.width, height: opened.height },
            doc: opened.edit,
            past: [],
            future: [],
            frame: null,
            fitFrame: null,
            histogram: null,
            zoom: 'fit',
            center: { x: 0.5, y: 0.5 },
            error: null,
          });
          requestView();
        } catch (e) {
          failed(e);
        }
      },

      setSetting(toolId, key, value, { gesture = false } = {}) {
        const s = get();
        const spec = s.tools.find((t) => t.id === toolId)?.settings.find((x) => x.key === key);
        if (!s.photo || !spec || valueOf(s.doc, toolId, spec) === value) return;
        const patch: EditPatch = { [toolId]: { [key]: value } };
        const label: HistoryLabel = { kind: 'set', toolId, key, value };
        const top = s.past[s.past.length - 1];
        const sameDrag = top?.open && top.label.kind === 'set' && top.label.toolId === toolId && top.label.key === key;
        const entry: HistoryEntry = sameDrag
          ? { ...top, patch, label, open: gesture }
          : { id: ++entryId, label, patch, inverse: inverseOf(s.doc, patch, s.tools), open: gesture };
        const past = sameDrag ? [...s.past.slice(0, -1), entry] : [...closeOpen(s.past), entry];
        set({ doc: applyPatch(s.doc, patch, s.tools), past, future: [] });
        send(patch, entry.id);
      },

      endGesture() {
        set({ past: closeOpen(get().past) });
      },

      commit(patch, label) {
        const s = get();
        if (!s.photo || isEmptyPatch(patch)) return;
        const entry: HistoryEntry = {
          id: ++entryId,
          label,
          patch,
          inverse: inverseOf(s.doc, patch, s.tools),
          open: false,
        };
        set({ doc: applyPatch(s.doc, patch, s.tools), past: [...closeOpen(s.past), entry], future: [] });
        send(patch, entry.id);
      },

      resetTool(toolId) {
        const s = get();
        const tool = s.tools.find((t) => t.id === toolId);
        if (tool) s.commit(resetPatch(s.doc, [tool]), { kind: 'resetTool', toolId });
      },

      resetSection(section) {
        const s = get();
        s.commit(
          resetPatch(
            s.doc,
            s.tools.filter((t) => t.section === section),
          ),
          { kind: 'resetSection', section },
        );
      },

      undo() {
        get().travel(get().past.length - 1);
      },

      redo() {
        get().travel(get().past.length + 1);
      },

      travel(length) {
        const s = get();
        const past = closeOpen(s.past);
        const target = Math.max(0, Math.min(past.length + s.future.length, length));
        if (target === past.length) return;
        let patch: EditPatch = {};
        let nextPast: HistoryEntry[];
        let nextFuture: HistoryEntry[];
        if (target < past.length) {
          // Newest first, so what is left is the oldest step's "before".
          const undone = past.slice(target).reverse();
          for (const entry of undone) patch = composePatches(patch, entry.inverse);
          nextPast = past.slice(0, target);
          nextFuture = [...s.future, ...undone];
        } else {
          const redone = s.future.slice(s.future.length - (target - past.length)).reverse();
          for (const entry of redone) patch = composePatches(patch, entry.patch);
          nextPast = [...past, ...redone];
          nextFuture = s.future.slice(0, s.future.length - redone.length);
        }
        set({ doc: applyPatch(s.doc, patch, s.tools), past: nextPast, future: nextFuture });
        send(patch);
      },

      setViewSize(width, height) {
        const v = get().viewSize;
        if (v && v.width === width && v.height === height) return;
        set({ viewSize: { width, height } });
        requestView();
      },

      setZoom(zoom, center) {
        set({ zoom, center: zoom === 'fit' ? { x: 0.5, y: 0.5 } : (center ?? get().center) });
        requestView();
      },

      setCenter(x, y) {
        const view = viewOf({ ...get(), center: { x, y } });
        if (!view) return;
        set({ center: { x: view.centerX, y: view.centerY } });
        requestView();
      },

      setMode: (mode) => set({ mode }),
      setSideTab: (sideTab) => set({ sideTab }),
      toggleSection: (section) => set({ collapsed: { ...get().collapsed, [section]: !get().collapsed[section] } }),

      addSnapshot(name) {
        set({ snapshots: [...get().snapshots, { name, doc: get().doc }] });
      },

      applySnapshot(index) {
        const s = get();
        const snapshot = s.snapshots[index];
        if (snapshot) s.commit(diffDocuments(s.doc, snapshot.doc, s.tools), { kind: 'snapshot', name: snapshot.name });
      },

      savePreset(name) {
        const s = get();
        set({ presets: [...s.presets, { name, patch: changesFromDefaults(s.doc, s.tools) }] });
      },

      applyPreset(index) {
        const s = get();
        const preset = s.presets[index];
        if (!preset) return;
        // Settings of tools this engine no longer has are left out rather than refused.
        const patch: EditPatch = {};
        for (const [toolId, values] of Object.entries(preset.patch)) {
          const tool = s.tools.find((t) => t.id === toolId);
          for (const [key, value] of Object.entries(values)) {
            if (tool?.settings.some((x) => x.key === key)) (patch[toolId] ??= {})[key] = value;
          }
        }
        const wanted = applyPatch(s.doc, patch, s.tools);
        s.commit(diffDocuments(s.doc, wanted, s.tools), { kind: 'preset', name: preset.name });
      },

      receive(event) {
        const s = get();
        if (event.event === 'error') {
          set({ error: event.message });
          return;
        }
        // Only the photo shown, and latest wins: anything for an older request than one seen is dropped.
        if (!s.photo || event.photoId !== s.photo.id || event.ticket < s.latestTicket) return;
        if (event.event === 'histogram') {
          set({ latestTicket: event.ticket, histogram: event.bins });
          return;
        }
        set({ latestTicket: event.ticket });
        void showFrame(event.ticket, latestIsFit);
      },
    };
  });
}
