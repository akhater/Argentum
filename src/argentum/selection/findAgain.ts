/**
 * A preset's or a paste's AI masks found again on each photo it reached. Ours.
 *
 * The preset or paste goes on with those masks' outlines taken out
 * (foundAgain.ts `withoutOutlines`); then each photo is asked for its own
 * (mods/selection_again.rs), one at a time, as Auto white balance measures
 * each pasted photo (autoWhiteBalance.ts `settleAutoWhiteBalance`). The open
 * photo goes first, its masks showing their working spinners; the others are
 * counted in a note and written to their sidecars as each is done. A photo
 * whose selection fails keeps that mask empty; nothing else is undone.
 */

import { invoke } from '@tauri-apps/api/core';
import { toast } from 'react-toastify';
import { Invokes } from '../../components/ui/AppProperties';
import { useEditorStore } from '../../store/useEditorStore';
import { useProcessStore } from '../../store/useProcessStore';
import type { Adjustments } from '../../utils/adjustments';
import { globalImageCache } from '../../utils/ImageLRUCache';
import { ag } from '../ag';
import type { Ask, Found } from '../ag.generated';
import { agText } from '../locales';
import { withOutlines } from './foundAgain';
import { ensureModels, geometryOf, type Models, modelsOf } from './selection';

type SetAdjustments = (update: (prev: Adjustments) => Adjustments) => void;

/** The asks whose models are here, each set asked for once. */
async function withModels(asks: ReadonlyArray<Ask>): Promise<Ask[]> {
  const ready = new Map<Models, boolean>();
  for (const ask of asks) {
    const models = modelsOf(ask.wanted.kind);
    if (!ready.has(models)) ready.set(models, await ensureModels(models));
  }
  return asks.filter((ask) => ready.get(modelsOf(ask.wanted.kind)));
}

/** The outlines into the open photo's edit, each one's cached bitmap dropped so the new one is sent. */
function fillOpenPhoto(found: ReadonlyArray<Found>, setAdjustments: SetAdjustments) {
  const { patchesSentToBackend } = useEditorStore.getState();
  for (const f of found) if (f.selection) patchesSentToBackend.delete(f.id);
  setAdjustments((prev) => ({ ...prev, masks: withOutlines(prev.masks ?? [], found) }));
}

async function onOpenPhoto(path: string, asks: Ask[], setAdjustments: SetAdjustments): Promise<Found[]> {
  const tasks = useProcessStore.getState();
  for (const ask of asks) tasks.registerAiTask(ask.id, 'foreground');
  try {
    const { adjustments } = useEditorStore.getState();
    const found = await ag('find_selections_again', { path, jsAdjustments: geometryOf(adjustments), asks });
    if (useEditorStore.getState().selectedImage?.path === path) fillOpenPhoto(found, setAdjustments);
    else await intoSidecar(path, found);
    return found;
  } finally {
    for (const ask of asks) tasks.unregisterAiTask(ask.id);
  }
}

/** The outlines into a photo that is not open, merged into what its sidecar holds now. */
async function intoSidecar(path: string, found: ReadonlyArray<Found>) {
  if (!found.some((f) => f.selection)) return;
  const meta = await invoke<{ adjustments?: Partial<Adjustments> | null }>(Invokes.LoadMetadata, { path });
  const masks = meta?.adjustments?.masks;
  if (!masks) return;
  globalImageCache.delete(path);
  await invoke(Invokes.ApplyAdjustmentsToPaths, { paths: [path], adjustments: { masks: withOutlines(masks, found) } });
}

async function onOtherPhoto(path: string, asks: Ask[], setAdjustments: SetAdjustments): Promise<Found[]> {
  const found = await ag('find_selections_again', { path, jsAdjustments: null, asks });
  if (useEditorStore.getState().selectedImage?.path === path) fillOpenPhoto(found, setAdjustments);
  else await intoSidecar(path, found);
  return found;
}

/**
 * Find `asks` again on every photo in `paths`, after the edit without their
 * outlines has reached them. `onOpen` hears what the open photo found.
 */
export async function findMasksAgain(
  paths: ReadonlyArray<string>,
  asks: ReadonlyArray<Ask>,
  setAdjustments: SetAdjustments,
  onOpen?: (found: Found[]) => void,
) {
  if (asks.length === 0 || paths.length === 0) return;
  const ready = await withModels(asks);
  if (ready.length === 0) return;

  const open = useEditorStore.getState().selectedImage?.path;
  if (open && paths.includes(open)) {
    try {
      const found = await onOpenPhoto(open, ready, setAdjustments);
      onOpen?.(found);
    } catch (err) {
      console.warn(`AI masks not found again on ${open}:`, err);
    }
  }

  const others = paths.filter((path) => path !== open);
  if (others.length === 0) return;
  const progress = (done: number) =>
    agText('masksFinding').replace('{done}', String(done)).replace('{count}', String(others.length));
  const note = toast.info(progress(0), { autoClose: false });
  let done = 0;
  let foundOn = 0;
  for (const path of others) {
    try {
      const found = await onOtherPhoto(path, ready, setAdjustments);
      if (found.some((f) => f.selection)) foundOn += 1;
    } catch (err) {
      console.warn(`AI masks not found again on ${path}:`, err);
    }
    done += 1;
    toast.update(note, { render: progress(done) });
  }
  toast.dismiss(note);
  if (foundOn > 0) toast.success(agText('masksFound').replace('{count}', String(foundOn)));
}
