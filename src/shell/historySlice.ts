// The edit and its history, as one slice of the shell's store. Every change
// is a patch sent to the engine, with its inverse kept for undo; the document
// is never snapshotted for undo.
//
// Changes show at once and are confirmed by the engine afterwards. The engine
// refuses a bad patch as a whole, so the slice keeps the edit the engine has
// accepted and the patches still on their way, and a refusal rebuilds the edit
// from those: the refused change is rolled back and nothing else is.

import type { EditDocument, EditPatch, Section, SettingValue } from './contract';
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
import type { EngineClient } from './engine/client';
import type { Shell, Slice } from './store';

export type HistoryLabel =
  | { kind: 'set'; toolId: string; key: string; value: SettingValue }
  | { kind: 'resetTool'; toolId: string }
  | { kind: 'resetSection'; section: Section }
  | { kind: 'resetAll' }
  | { kind: 'preset'; name: string }
  | { kind: 'snapshot'; name: string };

export interface HistoryEntry {
  readonly id: number;
  readonly label: HistoryLabel;
  readonly patch: EditPatch;
  readonly inverse: EditPatch;
  /** A slider drag still under way: its further ticks fold into this entry. */
  readonly open: boolean;
}

/** A patch sent and not yet answered, and the history entry it came from. */
interface PendingPatch {
  readonly patch: EditPatch;
  readonly entry?: number;
}

export interface HistoryState {
  doc: EditDocument;
  /** Steps applied, oldest first. */
  past: readonly HistoryEntry[];
  /** Steps undone, the next to redo last. */
  future: readonly HistoryEntry[];
  /** The edit as the engine has accepted it. */
  confirmed: EditDocument;
  /** Patches sent and not answered yet, in the order sent. */
  pending: readonly PendingPatch[];
  nextEntryId: number;
  snapshots: readonly { name: string; doc: EditDocument }[];
  presets: readonly { name: string; patch: EditPatch }[];
}

export interface HistoryActions {
  /** Start the history over on a newly opened photo's edit. */
  resetHistory(edit: EditDocument): void;
  /** One setting changed. With `gesture`, the ticks of one drag are one undo step. */
  setSetting(toolId: string, key: string, value: SettingValue, options?: { gesture?: boolean }): void;
  /** The drag that `setSetting(..., { gesture: true })` began is over. */
  endGesture(): void;
  /** Apply a patch as one undo step. */
  commit(patch: EditPatch, label: HistoryLabel): void;
  resetTool(toolId: string): void;
  resetSection(section: Section): void;
  resetAll(): void;
  undo(): void;
  redo(): void;
  /** Undo or redo until `length` steps are applied. */
  travel(length: number): void;
  addSnapshot(name: string): void;
  applySnapshot(index: number): void;
  savePreset(name: string): void;
  applyPreset(index: number): void;
}

/** The history with a drag left open closed, so the next change starts a step of its own. */
const closeOpen = (past: readonly HistoryEntry[]): readonly HistoryEntry[] =>
  past.length && past[past.length - 1].open ? [...past.slice(0, -1), { ...past[past.length - 1], open: false }] : past;

export const createHistorySlice =
  (client: EngineClient): Slice<HistoryState & HistoryActions> =>
  (set, get) => {
    /** Send a patch, and on refusal roll back that patch alone and drop its history entry. */
    const send = (patch: EditPatch, entry?: number) => {
      const { photo } = get();
      if (!photo || isEmptyPatch(patch)) return;
      const mine: PendingPatch = { patch, entry };
      set({ pending: [...get().pending, mine] });
      const request = get().requested;
      client.apply(photo.photoId, patch).then(
        (ticket) => {
          const s = get();
          if (s.photo?.photoId !== photo.photoId) return;
          set({ confirmed: applyPatch(s.confirmed, patch, s.tools), pending: s.pending.filter((p) => p !== mine) });
          s.recordTicket(ticket, request);
        },
        (e) => {
          const s = get();
          if (s.photo?.photoId !== photo.photoId) return;
          const pending = s.pending.filter((p) => p !== mine);
          const keep = (h: HistoryEntry) => entry === undefined || h.id !== entry;
          set({
            pending,
            doc: pending.reduce((doc, p) => applyPatch(doc, p.patch, s.tools), s.confirmed),
            past: s.past.filter(keep),
            future: s.future.filter(keep),
          });
          s.fail(e);
        },
      );
    };

    return {
      doc: emptyDocument(),
      past: [],
      future: [],
      confirmed: emptyDocument(),
      pending: [],
      nextEntryId: 1,
      snapshots: [],
      presets: [],

      resetHistory(edit) {
        set({ doc: edit, confirmed: edit, pending: [], past: [], future: [] });
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
          : { id: s.nextEntryId, label, patch, inverse: inverseOf(s.doc, patch, s.tools), open: gesture };
        const past = sameDrag ? [...s.past.slice(0, -1), entry] : [...closeOpen(s.past), entry];
        set({
          doc: applyPatch(s.doc, patch, s.tools),
          past,
          future: [],
          nextEntryId: sameDrag ? s.nextEntryId : s.nextEntryId + 1,
        });
        send(patch, entry.id);
      },

      endGesture() {
        set({ past: closeOpen(get().past) });
      },

      commit(patch, label) {
        const s = get();
        if (!s.photo || isEmptyPatch(patch)) return;
        const entry: HistoryEntry = {
          id: s.nextEntryId,
          label,
          patch,
          inverse: inverseOf(s.doc, patch, s.tools),
          open: false,
        };
        set({
          doc: applyPatch(s.doc, patch, s.tools),
          past: [...closeOpen(s.past), entry],
          future: [],
          nextEntryId: s.nextEntryId + 1,
        });
        send(patch, entry.id);
      },

      resetTool(toolId) {
        const s = get();
        const tool = s.tools.find((t) => t.id === toolId);
        if (tool) s.commit(resetPatch(s.doc, [tool]), { kind: 'resetTool', toolId });
      },

      resetSection(section) {
        const s = get();
        const tools = s.tools.filter((t) => t.section === section);
        s.commit(resetPatch(s.doc, tools), { kind: 'resetSection', section });
      },

      resetAll() {
        const s = get();
        s.commit(resetPatch(s.doc, s.tools), { kind: 'resetAll' });
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
        let nextPast: readonly HistoryEntry[];
        let nextFuture: readonly HistoryEntry[];
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
        const s: Shell = get();
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
    };
  };
