import { beforeEach, describe, expect, it, vi } from 'vitest';

/** Each photo's sidecar masks, and what each photo's sky selection finds. */
const backend = {
  sidecars: new Map<string, unknown>(),
  skies: new Map<string, string | null>(),
  asked: [] as Array<{ path: string; jsAdjustments: unknown }>,
};

/** What the code under test sends: an `ag` envelope or one of theirs. */
interface Call {
  name?: string;
  args: { path: string; jsAdjustments: unknown; asks: Array<{ id: string }> };
  path: string;
  paths: string[];
  adjustments: { masks: unknown };
}

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (command: string, args: Call) => {
    if (command === 'ag' && args.name === 'selection_models_status') return Promise.resolve({ installed: true });
    if (command === 'ag' && args.name === 'find_selections_again') {
      const { path, jsAdjustments, asks } = args.args;
      backend.asked.push({ path, jsAdjustments });
      return Promise.resolve(
        asks.map((ask) => {
          const sky = backend.skies.get(path);
          return {
            id: ask.id,
            selection: sky
              ? {
                  maskDataBase64: sky,
                  rotation: 0,
                  flipHorizontal: false,
                  flipVertical: false,
                  orientationSteps: 0,
                  found: true,
                }
              : null,
          };
        }),
      );
    }
    if (command === 'load_metadata')
      return Promise.resolve({ adjustments: { masks: backend.sidecars.get(args.path) } });
    if (command === 'apply_adjustments_to_paths') {
      for (const path of args.paths) backend.sidecars.set(path, args.adjustments.masks);
      return Promise.resolve();
    }
    return Promise.reject(new Error(`unexpected ${command}`));
  },
}));
vi.mock('../locales', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../locales')>()),
  agText: (key: string) => key,
}));
vi.mock('@tauri-apps/api/event', () => ({ listen: () => Promise.resolve(() => {}) }));
vi.mock('react-toastify', () => ({
  toast: Object.assign(() => 'note', {
    info: () => 'note',
    update: () => {},
    dismiss: () => {},
    success: () => {},
    error: () => {},
  }),
}));

import { useEditorStore } from '../../store/useEditorStore';
import type { Adjustments, MaskContainer } from '../../utils/adjustments';
import { findMasksAgain } from './findAgain';
import { asksOf, withoutOutlines } from './foundAgain';

const brushed = { id: 'brush', type: 'brush', parameters: { lines: [{ points: [{ x: 1, y: 2 }], brushSize: 40 }] } };
const masks = () =>
  [
    {
      id: 'sky',
      name: 'Sky',
      adjustments: { exposure: -1 },
      subMasks: [
        {
          id: 'sky1',
          type: 'ai-sky',
          parameters: { agSelection: 'sky', maskDataBase64: 'data:image/png;base64,FIRST' },
        },
        brushed,
      ],
    },
  ] as unknown as MaskContainer[];

const skyOf = (m: unknown) => (m as MaskContainer[])[0].subMasks[0].parameters.maskDataBase64;

describe('a preset with AI masks on several photos', () => {
  const asks = asksOf(masks());
  const stripped = withoutOutlines({ masks: masks() }, asks).masks;

  beforeEach(() => {
    backend.sidecars.clear();
    backend.skies.clear();
    backend.asked = [];
    useEditorStore.setState({ selectedImage: null } as never);
  });

  it('finds each photo its own sky, and a photo where it fails keeps the rest', async () => {
    for (const path of ['a.raf', 'b.raf', 'c.raf']) backend.sidecars.set(path, stripped);
    backend.skies.set('a.raf', 'data:image/png;base64,A');
    backend.skies.set('b.raf', 'data:image/png;base64,B');
    backend.skies.set('c.raf', null);

    await findMasksAgain(['a.raf', 'b.raf', 'c.raf'], asks, () => {});

    expect(skyOf(backend.sidecars.get('a.raf'))).toBe('data:image/png;base64,A');
    expect(skyOf(backend.sidecars.get('b.raf'))).toBe('data:image/png;base64,B');
    expect(skyOf(backend.sidecars.get('c.raf'))).toBeNull();
    for (const path of ['a.raf', 'b.raf', 'c.raf']) {
      const sidecar = backend.sidecars.get(path) as MaskContainer[];
      expect(JSON.stringify(sidecar[0].subMasks[1])).toBe(JSON.stringify(brushed));
      expect(sidecar[0].adjustments).toEqual({ exposure: -1 });
    }
    // Photos that are not open are read from their own files and sidecars.
    expect(backend.asked.every((a) => a.jsAdjustments === null)).toBe(true);
  });

  it('the open photo is found first, from the edit on screen', async () => {
    let edit = { exposure: 0, rotation: 0, masks: stripped } as unknown as Adjustments;
    useEditorStore.setState({ selectedImage: { path: 'open.raf' }, adjustments: edit } as never);
    backend.sidecars.set('other.raf', stripped);
    backend.skies.set('open.raf', 'data:image/png;base64,OPEN');
    backend.skies.set('other.raf', 'data:image/png;base64,OTHER');
    const heard: unknown[] = [];

    await findMasksAgain(
      ['other.raf', 'open.raf'],
      asks,
      (update) => (edit = update(edit)),
      (found) => heard.push(found),
    );

    expect(backend.asked.map((a) => a.path)).toEqual(['open.raf', 'other.raf']);
    expect(backend.asked[0].jsAdjustments).not.toHaveProperty('masks');
    expect(skyOf(edit.masks)).toBe('data:image/png;base64,OPEN');
    expect(skyOf(backend.sidecars.get('other.raf'))).toBe('data:image/png;base64,OTHER');
    expect(heard).toHaveLength(1);
  });

  it('nothing is asked for a preset without AI masks', async () => {
    await findMasksAgain(['a.raf'], [], () => {});
    expect(backend.asked).toEqual([]);
  });
});
