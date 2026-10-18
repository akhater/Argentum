// A test double of the engine: the same EngineClient as the real one, for the
// shell's tests and for `?shell=new` until the engine is wired into the app.
// Its tools are the engine's own descriptors, read from the generated
// fixtures. It keeps each photo's edit, refuses a bad patch as a whole and
// clamps values as the engine does (src-tauri/engine/src/engine.rs, tool.rs),
// hands out increasing tickets, renders latest-wins, and reports frame,
// histogram and error events in the contract's shapes.
//
// It is not a design mock: the shell sees only what the contract carries.

import fixtures from '../contract.fixtures.json';
import {
  EDIT_VERSION,
  type EditDocument,
  type EditPatch,
  type Event,
  type PhotoId,
  type RenderTicket,
  type SectionDescriptor,
  type SettingSpec,
  type SettingValue,
  type ToolDescriptor,
  type View,
} from '../contract';
import type { EngineClient, FramePixels } from './client';

/** The engine's sections and tools, exactly as it describes them. */
export const FAKE_SECTIONS = fixtures.sections as SectionDescriptor[];
export const FAKE_TOOLS = fixtures.tools as ToolDescriptor[];

/** Sample paths for the filmstrip. The fake paints a picture from each name. */
export const FAKE_PHOTOS = [
  'samples/IMG_0412.CR3',
  'samples/IMG_0415.CR3',
  'samples/IMG_0421.CR3',
  'samples/DSC_1180.NEF',
  'samples/DSC_1184.NEF',
  'samples/_R6A2207.CR3',
  'samples/_R6A2213.CR3',
  'samples/P1040562.RW2',
];

/** What a painter is asked for: one view of one photo. */
export interface PaintJob {
  ticket: RenderTicket;
  path: string;
  width: number;
  height: number;
  view: View;
  edit: EditDocument;
}

/** Pixels and their 256 luminance bins, or null when a newer job replaced this one. */
export type FakePainter = (job: PaintJob) => Promise<{ pixels: FramePixels; bins: number[] } | null>;

export type FakeCall =
  | { method: 'sections' }
  | { method: 'tools' }
  | { method: 'open'; path: string }
  | { method: 'apply'; photo: PhotoId; patch: EditPatch; ticket: RenderTicket }
  | { method: 'view'; photo: PhotoId; view: View; ticket: RenderTicket };

export interface FakeEngine extends EngineClient {
  /** Every call that was accepted, in order. */
  readonly calls: FakeCall[];
  /** Deliver an event as if the engine had sent it. */
  emit(event: Event): void;
  /** The engine's copy of a photo's edit. */
  editOf(photo: PhotoId): EditDocument | undefined;
}

export interface FakeOptions {
  tools?: ToolDescriptor[];
  /** Makes the pixels. Without one, frames are flat grey of the asked size. */
  paint?: FakePainter;
  /** Render on its own after each apply and view. Off for tests that send events by hand. */
  autoFrames?: boolean;
}

/** A stable size per path, landscape or portrait, as a camera would give. */
function sizeOf(path: string): { width: number; height: number } {
  let h = 0;
  for (const c of path) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  return h % 4 === 0 ? { width: 4000, height: 6000 } : { width: 6000, height: 4000 };
}

/** The value as the engine keeps it, or why it refuses it. */
function checked(spec: SettingSpec, value: SettingValue | null): SettingValue | Error {
  if (value === null) return spec.default;
  switch (spec.kind) {
    case 'bool':
      return typeof value === 'boolean' ? value : new Error(`${spec.key} takes true or false`);
    case 'choice':
      return typeof value === 'string' && spec.choices?.some((c) => c.value === value)
        ? value
        : new Error(`${spec.key} has no choice ${String(value)}`);
    case 'int':
    case 'float': {
      if (typeof value !== 'number' || !Number.isFinite(value)) return new Error(`${spec.key} takes a number`);
      const n = spec.kind === 'int' ? Math.round(value) : value;
      return Math.min(spec.max ?? Infinity, Math.max(spec.min ?? -Infinity, n));
    }
  }
}

export function createFakeEngine(options: FakeOptions = {}): FakeEngine {
  const tools = options.tools ?? FAKE_TOOLS;
  const autoFrames = options.autoFrames ?? true;
  const listeners = new Set<(event: Event) => void>();
  const photos = new Map<PhotoId, { path: string; width: number; height: number; edit: EditDocument }>();
  const ids = new Map<string, PhotoId>();
  const views = new Map<PhotoId, View>();
  const calls: FakeCall[] = [];
  let ticket = 0;
  let latest: { ticket: RenderTicket; pixels: FramePixels | null; size: { width: number; height: number } } | null =
    null;

  const emit = (event: Event) => listeners.forEach((l) => l(event));
  const later = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

  const photoOf = (id: PhotoId) => {
    const p = photos.get(id);
    if (!p) throw new Error(`no photo ${id} is open`);
    return p;
  };

  // A newer ticket cancels every older one: a job superseded before its turn never renders.
  async function render(t: RenderTicket, id: PhotoId) {
    await later();
    if (t !== ticket) return;
    const p = photos.get(id);
    const view = views.get(id);
    if (!p || !view) return;
    try {
      const painted = options.paint ? await options.paint({ ticket: t, ...p, view }) : undefined;
      if (painted === null || t !== ticket) return;
      const size = painted ? { width: painted.pixels.width, height: painted.pixels.height } : view;
      latest = { ticket: t, pixels: painted?.pixels ?? null, size };
      emit({ event: 'frame', ticket: t, photoId: id, width: size.width, height: size.height });
      emit({ event: 'histogram', ticket: t, photoId: id, bins: painted?.bins ?? new Array(256).fill(0) });
    } catch (e) {
      emit({ event: 'error', ticket: t, message: String(e) });
    }
  }

  return {
    calls,
    emit,
    editOf: (id) => photos.get(id)?.edit,

    async sections() {
      calls.push({ method: 'sections' });
      await later();
      return structuredClone(FAKE_SECTIONS);
    },

    async tools() {
      calls.push({ method: 'tools' });
      await later();
      return structuredClone(tools);
    },

    async open(path) {
      calls.push({ method: 'open', path });
      await later();
      let id = ids.get(path);
      if (id === undefined) {
        id = ids.size + 1;
        ids.set(path, id);
        const edit: EditDocument = { version: EDIT_VERSION, tools: {} };
        for (const t of tools) {
          edit.tools[t.id] = { v: t.version, settings: Object.fromEntries(t.settings.map((s) => [s.key, s.default])) };
        }
        photos.set(id, { path, ...sizeOf(path), edit });
      }
      const p = photoOf(id);
      return { photoId: id, width: p.width, height: p.height, edit: structuredClone(p.edit) };
    },

    async apply(id, patch) {
      const p = photoOf(id);
      // All or nothing: the first bad value refuses the whole patch.
      const next = structuredClone(p.edit);
      for (const [toolId, values] of Object.entries(patch)) {
        const tool = tools.find((t) => t.id === toolId);
        if (!tool) throw new Error(`no tool ${toolId}`);
        for (const [key, value] of Object.entries(values)) {
          const spec = tool.settings.find((s) => s.key === key);
          if (!spec) throw new Error(`${toolId} has no setting ${key}`);
          const v = checked(spec, value);
          if (v instanceof Error) throw v;
          (next.tools[toolId] ??= { v: tool.version, settings: {} }).settings[key] = v;
        }
      }
      p.edit = next;
      const t = ++ticket;
      calls.push({ method: 'apply', photo: id, patch, ticket: t });
      if (autoFrames && views.has(id)) void render(t, id);
      return t;
    },

    async view(id, view) {
      photoOf(id);
      views.set(id, view);
      const t = ++ticket;
      calls.push({ method: 'view', photo: id, view, ticket: t });
      if (autoFrames) void render(t, id);
      return t;
    },

    async frame(t) {
      if (!latest || latest.ticket !== t) return null;
      const { width, height } = latest.size;
      latest.pixels ??= { width, height, data: new Uint8ClampedArray(width * height * 4).fill(128) };
      return latest.pixels;
    },

    subscribe(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
  };
}
